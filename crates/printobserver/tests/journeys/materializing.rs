//! An image is a path that opens, or a failure of its own.
//!
//! # Why the file has to be made to go
//!
//! The supervisor answers a path only when the file is there, and on one host
//! its filesystem and this program's are the same one — so a colocated tier
//! cannot otherwise reach the failure a client somewhere else meets every time.
//! The proxy hands the answer on once the file it names has gone, which is what
//! this program meets when it runs where that path does not resolve. The
//! condition the program tests is whether the path exists **here**, and
//! deliberately not whether the configured address is somewhere else: a
//! supervisor on another host sharing a filesystem hands back a path that
//! genuinely does open.

use printobserver::failure::Exit;
use printobserver_server::{Answer, OPERATIONS};
use printobserver_types::serde_json::Value;

use crate::traced::Ran;
use crate::walk;
use crate::world::World;

use super::{formats, running};

/// The commands whose answer carries a materialized image path.
///
/// Read off the server's own declared response shapes rather than off a list
/// kept here, so a route that later grows such a path cannot fall outside this.
fn commands_carrying_a_path() -> Vec<(String, &'static str)> {
    OPERATIONS
        .iter()
        .filter_map(|operation| {
            operation
                .image_path_field
                .map(|field| (printobserver::surface::command_for(operation.name), field))
        })
        .collect()
}

/// The arguments one such command is driven with.
fn driven(world: &World, command: &str) -> Vec<String> {
    let found = walk::walk(world)
        .into_iter()
        .find(|one| one.command.name == command)
        .unwrap_or_else(|| panic!("this walk drives no `{command}`"));
    super::failures::succeeding(&found)
}

/// The path a materialized answer prints opens, and its contents are the image.
pub fn an_image_is_a_path_that_opens(world: &World) {
    world.freshen_the_image();
    let carrying = commands_carrying_a_path();
    assert!(
        !carrying.is_empty(),
        "no declared answer carries a materialized image path, so this rule guards a \
         boundary nothing is on"
    );
    for (command, field) in carrying {
        let arguments = driven(world, &command);
        let asked: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let answer = running::read(world, &asked);
        let printed = answer
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("`{command}` printed no `{field}`: {answer}"));

        let bytes = std::fs::read(printed).unwrap_or_else(|error| {
            panic!("`{command}` printed {printed}, which does not open: {error}")
        });
        assert_eq!(
            digest_of(&bytes),
            World::image_digest(),
            "the file `{command}` printed is not the image the record declares"
        );
    }
}

/// The digest of some bytes, spelled the way an image record spells one.
fn digest_of(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// A path that names no file here is its own failure, and the rest is printed.
pub fn a_path_that_names_no_file_here_is_its_own_failure(world: &World) {
    for (command, field) in commands_carrying_a_path() {
        let ran = losing_the_image(world, &command, &[]);

        assert_eq!(
            ran.code,
            Some(i32::from(Exit::ImageElsewhere.status())),
            "`{command}` did not answer a path that names no file here as its own \
             failure: {}",
            ran.said()
        );
        let said = ran.said();
        assert!(
            said.contains(&world.proxy.url()),
            "`{command}` did not name the address the path is a file on: {said}"
        );
        assert!(
            said.contains("Run this command on that host")
                && said.contains("point this program at a supervisor serving on this one"),
            "`{command}` did not name both ways out: {said}"
        );
        assert!(
            !ran.out.contains(&world.image_path().display().to_string()),
            "`{command}` printed a path that names no file here as though it were one: {}",
            ran.out
        );
        the_rest_of_the_answer_is_printed(&ran, &command, field);
    }
}

/// The other fields of the answer are printed, so a decision that does not need
/// the image is still one the caller can take.
fn the_rest_of_the_answer_is_printed(ran: &Ran, command: &str, field: &str) {
    let said = super::answers::labelled(&ran.out);
    assert!(
        said.keys()
            .any(|at| at.contains("print") || at.contains("record")),
        "`{command}` printed none of the rest of the answer it did receive: {}",
        ran.out
    );
    assert!(
        said.contains_key(&format!(
            "{field}{}",
            printobserver::client::UNAVAILABLE_SUFFIX
        )),
        "`{command}` printed nothing in place of the path: {}",
        ran.out
    );
}

/// Both renderings of that failure, held to everything both are held to.
pub fn both_renderings_of_a_path_that_names_no_file(world: &World) {
    for (command, _) in commands_carrying_a_path() {
        let operation = printobserver_server::operation(&command.replace('-', "_"))
            .expect("the server serves this");
        // One fresh alert for the pair rather than one each: a context answer
        // carries the print's own recent events, and two runs either side of a
        // new one would be being compared across two different histories.
        world.freshen_the_image();
        let machine = losing(world, &command, &["--json"]);
        let plain = losing(world, &command, &[]);
        formats::the_two_renderings_agree(&machine, &plain, operation, Answer::Success);
    }
}

/// One run of one command, with the file it names going before the answer
/// reaches it.
fn losing_the_image(world: &World, command: &str, also: &[&str]) -> Ran {
    world.freshen_the_image();
    losing(world, command, also)
}

/// The same, against the alert that is already the print's most recent.
fn losing(world: &World, command: &str, also: &[&str]) -> Ran {
    let arguments = driven(world, command);
    let mut asked: Vec<&str> = arguments.iter().map(String::as_str).collect();
    asked.extend_from_slice(also);
    world.proxy.losing_the_file(Some(world.image_path()));
    let ran = running::command(world, &asked);
    world.proxy.losing_the_file(None);
    world.restore_the_image();
    ran
}
