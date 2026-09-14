//! Finding the print to act on, the way a person does: start a print at the
//! printer, ask `printobserver prints`, and read the one it names.
//!
//! Every step is this program run as a subprocess against the supervisor the
//! server command started, over a stood-in machine. The print this world
//! seeded is one an earlier alert opened; ending it is what leaves the machine's
//! job with no print open for it, which is the state a print just started at
//! the printer is in.

use std::io::{Read as _, Write as _};
use std::time::{Duration, Instant};

use printobserver_core::store::PrintStore as _;
use printobserver_types::serde_json::{Value, json};

use crate::running;
use crate::world::{SECRET, World};

/// `Obico`'s own identifier for the print this journey's alert is about.
const OBICO_PRINT: i64 = 5150;

/// How long the supervisor is given to handle one alert.
const HANDLING: Duration = Duration::from_secs(60);

fn prints(world: &World) -> Value {
    running::read(world, &["prints"])
}

/// End this world's own print, as a terminal state an event reported would.
///
/// Through a second connection to the store the running supervisor holds, which
/// the schema's write-ahead log is what allows.
fn end_the_print(world: &World) {
    let store = printobserver_store_sqlite::SqliteStore::open(world.root.path().join("state"))
        .expect("the store opens");
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(store.end_print(
            world.print_id.parse().expect("a print identifier"),
            printobserver_core::records::PrinterState::Operational,
            printobserver_types::Timestamp::now(),
            "this journey finished it".to_owned(),
        ))
        .expect("the print ends");
}

/// Post the committed `Obico` failure alert about one of `Obico`'s prints to
/// the supervisor's own ingress, and answer the status line it came under.
///
/// Its image names a port nothing listens on, so fetching it fails at once and
/// is recorded; what this journey is about is which print the alert lands on.
fn post_a_failure_alert(world: &World, obico_print_id: i64) -> String {
    let mut alert: Value = printobserver_types::serde_json::from_str(include_str!(
        "../../../printobserver-obico/samples/obico/failure-alert.json"
    ))
    .expect("the committed sample is JSON");
    alert["print"]["id"] = json!(obico_print_id);
    alert["img_url"] = json!("http://127.0.0.1:9/snapshot.jpg");
    let body = alert.to_string();
    let address = world.proxy.address;
    let mut stream = std::net::TcpStream::connect(address).expect("the supervisor is reachable");
    write!(
        stream,
        "POST {}?{}={SECRET} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        printobserver_server::INGRESS_PATH,
        printobserver_server::TOKEN_PARAM,
        body.len()
    )
    .expect("the alert is written");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer is read");
    answer.lines().next().unwrap_or_default().to_owned()
}

/// Every identifier a listing carries, in the order it carries them.
fn listed(answer: &Value) -> Vec<String> {
    answer["prints"]
        .as_array()
        .unwrap_or_else(|| panic!("`prints` answered no list: {answer}"))
        .iter()
        .map(|print| print["id"].as_str().expect("an identifier").to_owned())
        .collect()
}

/// The whole path from a print started at the printer to reading and acting on
/// it: the listing names it, lists it newest first, adopts it once, still
/// answers when the machine cannot be read, and the first alert about it joins
/// it rather than opening a second.
pub fn a_print_started_at_the_printer_is_found_read_and_joined_by_its_alert(world: &World) {
    let before = prints(world);
    assert_eq!(
        before["active"],
        json!(world.print_id),
        "the open print carrying the running file was not named active: {before}"
    );
    assert_eq!(listed(&before), vec![world.print_id.clone()], "{before}");

    end_the_print(world);
    let found = prints(world);
    let ids = listed(&found);
    assert_eq!(
        ids.len(),
        2,
        "the running job with no open print was not adopted as one print: {found}"
    );
    assert_eq!(
        ids[1], world.print_id,
        "the listing is not most recently opened first: {found}"
    );
    let adopted = ids[0].clone();
    assert_eq!(found["active"], json!(adopted), "{found}");
    assert_eq!(
        found["prints"][0]["file_name"],
        json!(crate::machine::RUNNING_FILE),
        "{found}"
    );
    assert!(
        found["prints"][0].get("provider_print_id").is_none(),
        "{found}"
    );
    assert_eq!(
        prints(world),
        found,
        "reading the prints again while the same job runs changed them"
    );

    let context = running::read(world, &["context", "--print-id", &adopted]);
    assert_eq!(
        context["context"]["print"]["id"],
        json!(adopted),
        "the first read by the identifier the listing gave is about another print: {context}"
    );

    assert!(world.machine_refuses(true), "a stood-in machine can refuse");
    let unreadable = prints(world);
    world.machine_refuses(false);
    assert_eq!(listed(&unreadable), ids, "{unreadable}");
    assert!(
        unreadable.get("active").is_none(),
        "a machine that could not be read had a job named active: {unreadable}"
    );

    let answered = post_a_failure_alert(world, OBICO_PRINT);
    assert_eq!(
        answered.split_whitespace().nth(1),
        Some("202"),
        "the ingress did not take the alert: {answered}"
    );
    let deadline = Instant::now() + HANDLING;
    let joined = loop {
        let now = prints(world);
        if now["prints"][0]["provider_print_id"] == json!(OBICO_PRINT) || Instant::now() > deadline
        {
            break now;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(
        joined["prints"][0]["provider_print_id"],
        json!(OBICO_PRINT),
        "the alert did not attach its identifier to the adopted print: {joined}"
    );
    assert_eq!(
        listed(&joined),
        ids,
        "the alert opened a print beside the one adopted for its job: {joined}"
    );
    assert_eq!(joined["active"], json!(adopted), "{joined}");
}
