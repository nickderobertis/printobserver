//! Reading one body the producer posted into this system's own vocabulary.
//!
//! Nothing here supplies a default for a field the contracts declare the
//! producer sends. A body that omits one, or carries one whose value is of the
//! wrong kind, is refused as [`VisionError::Malformed`] carrying its bytes —
//! those two are the cases an implementation defaults its way past while a test
//! over an unparseable body goes on passing, and they are the ones that
//! silently lose a real alert.

use core::time::Duration;

use chrono::DateTime;
use printobserver_types::serde_json::{self, Value};
use printobserver_types::{EventBody, RawBytes, Timestamp};
use printobserver_vision_api::{NormalizedAlert, ProviderPrint, VisionError};

use crate::events::{
    ObicoFailureAlertPayload, ObicoNotificationType, ObicoPrinterNotificationPayload, obico_source,
};
use crate::wire::{
    ObicoEventType, ObicoFailureAlert, ObicoPrintInfo, ObicoPrinterNotification, ObicoTimestamp,
};

/// The producer's own spelling of the `type` a failure alert's event carries.
///
/// Which shape a body is has to be decided before it is parsed as either, and
/// this is the one field that says. `the_failure_spelling_is_the_contract_s`
/// holds it to what the contract's own type serializes as.
const FAILURE_EVENT_TYPE: &str = "PrintFailure";

/// The pointer into a body naming which of the producer's shapes it is.
const EVENT_TYPE_POINTER: &str = "/event/type";

/// Refuse one body, carrying its bytes so that it is not lost.
fn malformed(detail: impl Into<String>, body: &RawBytes) -> VisionError {
    VisionError::Malformed {
        detail: detail.into(),
        raw: body.clone(),
    }
}

/// The instant a Unix timestamp number names, to the nanosecond.
///
/// [`Duration`] does the conversion because it refuses a non-finite or
/// unrepresentable count rather than truncating one, and answers whole seconds
/// and a sub-second nanosecond count separately, so nothing here casts a float
/// to an integer. A negative count — an instant before the Unix epoch, which is
/// not one a print carries — is refused along with the rest.
fn instant_named_by(seconds: f64) -> Option<Timestamp> {
    let elapsed = Duration::try_from_secs_f64(seconds).ok()?;
    let whole = i64::try_from(elapsed.as_secs()).ok()?;
    DateTime::from_timestamp(whole, elapsed.subsec_nanos()).map(Timestamp::from)
}

/// One of the producer's timestamps, in each of the three states it sends.
///
/// A number becomes the instant it names; the empty string and an omitted field
/// each become absent. None of the three becomes an epoch date, and a number
/// that names no representable instant is a refusal rather than a fourth state.
fn instant_of(
    reported: Option<ObicoTimestamp>,
    field: &str,
    body: &RawBytes,
) -> Result<Option<Timestamp>, VisionError> {
    match reported {
        None | Some(ObicoTimestamp::NotReported) => Ok(None),
        Some(ObicoTimestamp::Seconds(seconds)) => {
            instant_named_by(seconds).map(Some).ok_or_else(|| {
                malformed(
                    format!(
                        "`print.{field}` is {seconds}, which names no instant this system can hold"
                    ),
                    body,
                )
            })
        }
    }
}

/// Everything one body says about the print it is about.
struct Correlation {
    /// Obico's own identifier for the print.
    obico_print_id: Option<i64>,
    /// The file being printed, as the producer reported it.
    file_name: Option<String>,
    /// When the print started, when the producer reported an instant.
    started_at: Option<Timestamp>,
    /// When the print ended, when the producer reported an instant.
    ended_at: Option<Timestamp>,
}

impl Correlation {
    /// The print this body is about, as the supervision domain correlates on it.
    fn provider_print(&self) -> Option<ProviderPrint> {
        self.obico_print_id.map(|id| ProviderPrint {
            id,
            file_name: self.file_name.clone(),
        })
    }

    /// What a body carrying no print says: nothing, in all four.
    const fn about_no_print() -> Self {
        Self {
            obico_print_id: None,
            file_name: None,
            started_at: None,
            ended_at: None,
        }
    }

    /// What a body's `print` object says.
    fn of(print: ObicoPrintInfo, body: &RawBytes) -> Result<Self, VisionError> {
        Ok(Self {
            obico_print_id: Some(print.id),
            file_name: Some(print.filename),
            started_at: instant_of(print.started_at, "started_at", body)?,
            ended_at: instant_of(print.ended_at, "ended_at", body)?,
        })
    }
}

/// The normalized spelling of one of the producer's notification types.
const fn notification_type_of(reported: ObicoEventType) -> ObicoNotificationType {
    match reported {
        ObicoEventType::PrintStarted => ObicoNotificationType::Started,
        ObicoEventType::PrintDone => ObicoNotificationType::Done,
        ObicoEventType::PrintCancelled => ObicoNotificationType::Cancelled,
        ObicoEventType::PrintPaused => ObicoNotificationType::Paused,
        ObicoEventType::PrintResumed => ObicoNotificationType::Resumed,
        ObicoEventType::FilamentChange => ObicoNotificationType::FilamentChange,
        ObicoEventType::HeaterCooledDown => ObicoNotificationType::HeaterCooled,
        ObicoEventType::HeaterTargetReached => ObicoNotificationType::HeaterTarget,
    }
}

/// What one body was read into: its event, the image it names, and its print.
struct Read {
    /// The event, under this adapter's own kind.
    body: EventBody,
    /// The image the body names, when it names one.
    image_url: Option<String>,
    /// The print the body is about, when it is about one.
    print: Option<ProviderPrint>,
}

/// Render one payload as a body, which a payload this adapter declares never
/// refuses.
fn body_of<P: printobserver_types::EventPayload>(
    payload: &P,
    body: &RawBytes,
) -> Result<EventBody, VisionError> {
    EventBody::of(payload)
        .map_err(|error| malformed(format!("the payload would not render: {error}"), body))
}

/// Read one failure alert.
fn read_failure_alert(parsed: Value, body: &RawBytes) -> Result<Read, VisionError> {
    let alert: ObicoFailureAlert = serde_json::from_value(parsed)
        .map_err(|error| malformed(format!("the body is not a failure alert: {error}"), body))?;
    let image_url = alert.img_url;
    let correlation = Correlation::of(alert.print, body)?;
    let payload = ObicoFailureAlertPayload {
        is_warning: alert.event.is_warning,
        print_paused: alert.event.print_paused,
        obico_print_id: correlation.obico_print_id,
        file_name: correlation.file_name.clone(),
        started_at: correlation.started_at,
        ended_at: correlation.ended_at,
    };
    Ok(Read {
        body: body_of(&payload, body)?,
        image_url: Some(image_url),
        print: correlation.provider_print(),
    })
}

/// Read one printer notification, in either of the two forms it is sent in.
fn read_printer_notification(parsed: Value, body: &RawBytes) -> Result<Read, VisionError> {
    let notification: ObicoPrinterNotification =
        serde_json::from_value(parsed).map_err(|error| {
            malformed(
                format!("the body is not a printer notification: {error}"),
                body,
            )
        })?;
    let correlation = match notification.print {
        Some(print) => Correlation::of(print, body)?,
        None => Correlation::about_no_print(),
    };
    let payload = ObicoPrinterNotificationPayload {
        notification_type: notification_type_of(notification.event.event_type),
        obico_print_id: correlation.obico_print_id,
        file_name: correlation.file_name.clone(),
        started_at: correlation.started_at,
        ended_at: correlation.ended_at,
    };
    Ok(Read {
        body: body_of(&payload, body)?,
        image_url: notification.img_url,
        print: correlation.provider_print(),
    })
}

/// Read one received body into an event under this adapter's own kind.
///
/// `content_type` is what the caller was told the body is, and it appears in
/// the refusal when the body does not parse — the one thing it is good for
/// here, since the producer's own plugin posts JSON whatever it declares.
pub(crate) fn read(
    body: &RawBytes,
    content_type: Option<&str>,
    received_at: Timestamp,
) -> Result<NormalizedAlert, VisionError> {
    let declared = content_type.unwrap_or("no content type");
    let parsed: Value = serde_json::from_slice(body.as_slice()).map_err(|error| {
        malformed(
            format!("the body arrived as {declared} and is not JSON: {error}"),
            body,
        )
    })?;
    let shape = parsed
        .pointer(EVENT_TYPE_POINTER)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            malformed(
                format!("the body carries no string at {EVENT_TYPE_POINTER} naming its shape"),
                body,
            )
        })?;
    let read = if shape == FAILURE_EVENT_TYPE {
        read_failure_alert(parsed, body)?
    } else {
        read_printer_notification(parsed, body)?
    };
    Ok(NormalizedAlert {
        source: obico_source(),
        received_at,
        body: read.body,
        raw: body.clone(),
        image_url: read.image_url,
        print: read.print,
    })
}

#[cfg(test)]
mod tests {
    use super::{FAILURE_EVENT_TYPE, instant_named_by};
    use crate::wire::ObicoFailureEventType;

    /// The spelling this module dispatches on is the contract's own.
    #[test]
    fn the_failure_spelling_is_the_contract_s() {
        let serialized =
            printobserver_types::serde_json::to_value(ObicoFailureEventType::PrintFailure)
                .expect("a fieldless enum serializes");
        assert_eq!(serialized.as_str(), Some(FAILURE_EVENT_TYPE));
    }

    /// A count of seconds that names no instant is answered as none.
    #[test]
    fn a_count_naming_no_instant_is_none() {
        assert_eq!(instant_named_by(f64::NAN), None);
        assert_eq!(instant_named_by(-1.0), None);
        assert_eq!(instant_named_by(f64::MAX), None);
    }
}
