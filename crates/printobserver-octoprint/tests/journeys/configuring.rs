//! Configuring the adapter: what a base URL may be, and what it may not.

use core::time::Duration;

use printobserver_octoprint::{
    ConfigError, Endpoint, FanSupport, OctoPrintConfig, OctoPrintPrinter, SCHEME,
};

/// A base URL naming host, port and a prefix reaches every path under it.
#[test]
fn a_base_url_with_a_prefix_prefixes_every_path() {
    let endpoint = Endpoint::parse("http://printer.local:5000/octoprint/").expect("an endpoint");

    assert_eq!(endpoint.host(), "printer.local");
    assert_eq!(endpoint.port(), 5000);
    assert_eq!(endpoint.path("/api/job"), "/octoprint/api/job");
    assert_eq!(endpoint.to_string(), "http://printer.local:5000/octoprint");
}

/// A base URL naming no port is reached on the default one.
#[test]
fn a_base_url_with_no_port_takes_the_default_one() {
    let endpoint = Endpoint::parse("http://printer.local").expect("an endpoint");

    assert_eq!(endpoint.port(), 80);
    assert_eq!(endpoint.path("/api/printer"), "/api/printer");
}

/// The one scheme this adapter speaks is the one it refuses everything else for.
#[test]
fn a_base_url_spelled_with_another_scheme_is_refused() {
    let refusal = Endpoint::parse("https://printer.local:5000").expect_err("a refusal");

    assert_eq!(
        refusal,
        ConfigError::UnsupportedScheme {
            base_url: "https://printer.local:5000".to_owned(),
        }
    );
    assert!(refusal.to_string().contains(SCHEME), "{refusal}");
}

/// A base URL naming no host names no instance.
#[test]
fn a_base_url_naming_no_host_is_refused() {
    assert_eq!(
        Endpoint::parse("http://:5000").expect_err("a refusal"),
        ConfigError::NoHost {
            base_url: "http://:5000".to_owned(),
        }
    );
}

/// A base URL whose port is not a port names no instance.
#[test]
fn a_base_url_whose_port_is_not_a_number_is_refused() {
    assert_eq!(
        Endpoint::parse("http://printer.local:wat").expect_err("a refusal"),
        ConfigError::BadPort {
            port: "wat".to_owned(),
        }
    );
}

/// An instance with authentication enabled refuses an empty key, so the
/// configuration does too.
#[test]
fn an_empty_api_key_is_refused() {
    let refusal = OctoPrintConfig::new("http://printer.local:5000", "   ").expect_err("a refusal");

    assert_eq!(refusal, ConfigError::EmptyApiKey);
    assert!(!refusal.to_string().is_empty(), "the refusal says nothing");
}

/// The timeout and the fan statement are the configuration's, and are readable.
#[test]
fn the_timeout_and_the_fan_statement_are_carried() {
    let config = OctoPrintConfig::new("http://printer.local:5000", "key")
        .expect("a configuration")
        .with_timeout(Duration::from_millis(250))
        .with_fan(FanSupport::Absent);

    assert_eq!(config.timeout(), Duration::from_millis(250));
    assert_eq!(config.fan(), FanSupport::Absent);
    assert_eq!(config.endpoint().host(), "printer.local");
    assert_eq!(config.api_key().expose(), "key");
}

/// Every error a configuration can carry says something a reader can act on.
#[test]
fn every_configuration_refusal_says_what_is_wrong() {
    let refusals = [
        ConfigError::EmptyApiKey,
        ConfigError::UnsupportedScheme {
            base_url: "ftp://x".to_owned(),
        },
        ConfigError::NoHost {
            base_url: "http://".to_owned(),
        },
        ConfigError::BadPort {
            port: "x".to_owned(),
        },
    ];
    for refusal in refusals {
        assert!(!refusal.to_string().is_empty(), "{refusal:?} says nothing");
    }
}

/// The fan statement says which of the two it is.
#[test]
fn the_fan_statement_says_which_it_is() {
    assert_eq!(FanSupport::Commandable.to_string(), "commandable");
    assert_eq!(FanSupport::Absent.to_string(), "absent");
    assert_eq!(FanSupport::default(), FanSupport::Commandable);
}

/// The adapter carries the configuration it was built from, so a caller
/// composing it can read back what it is talking to.
#[test]
fn the_adapter_carries_the_configuration_it_was_built_from() {
    let config = OctoPrintConfig::new("http://printer.local:5000", "key")
        .expect("a configuration")
        .with_fan(FanSupport::Absent);

    let printer = OctoPrintPrinter::new(config.clone());

    assert_eq!(printer.config(), &config);
    assert_eq!(
        printer.config().endpoint().to_string(),
        "http://printer.local:5000"
    );
}
