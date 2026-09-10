//! What makes the all-operation walk's own equality worth having.
//!
//! `live.rs` asserts that what each method answered carries, field for field,
//! what the **real** supervisor sent. An assertion nothing can fail is not one,
//! so two clients that a walk over emitted values would never catch are driven
//! through that same comparison here and are asserted to be refused.
//!
//! Both are about the one thing this system will not do: neither of them puts
//! image bytes in an answer *legitimately* — they put them where a client with
//! a defect would, at a field of a generated response type that declares
//! nothing of the kind, which is exactly the shape no probe guessing at
//! encodings would find. One carries a base64 of the image in an existing
//! string field; the other carries the bytes themselves, as a byte sequence
//! rather than as any rendering of them into text.
//!
//! # Both variants are real clients
//!
//! Each is the published client with one thing done to what it answers, which
//! is what a defect is. The comparison is the committed one — `live::matches`,
//! the function the generated walk calls — so what is proven is that walk's
//! own assertion rather than a copy of it.

#[path = "support/live.rs"]
mod live;
#[path = "support/supervisor.rs"]
mod supervisor;

use std::collections::BTreeMap;

use base64::Engine as _;
use printobserver_sdk::{Actor, Client, ImageAnswer, JobManifest};
use serde_json::Value;

/// The stored image's own bytes, opened at the path the supervisor answered.
///
/// # Panics
///
/// Panics when the supervisor answered no path, or the path opens nothing.
fn bytes_of(answered: &ImageAnswer) -> Vec<u8> {
    let path = answered
        .path
        .as_ref()
        .expect("the supervisor answered a path on its own host");
    std::fs::read(path).expect("the image at the answered path opens")
}

/// A client that writes a base64 of the stored image's bytes into an existing
/// string field of a generated response type.
///
/// The field is the record's own digest, which is a string the contracts
/// declare and a caller reads — so nothing about the answer's *shape* has
/// changed, and only what it carries has.
fn base64_of_the_image_into_a_string_field(answered: &ImageAnswer) -> ImageAnswer {
    let mut variant = answered.clone();
    variant.record.sha256 = base64::engine::general_purpose::STANDARD.encode(bytes_of(answered));
    variant
}

/// A client that returns the image's own bytes in place of a field's value.
///
/// The bytes themselves, and no rendering of them into text: what this puts
/// where the content type belongs is exactly what a client whose own answer
/// type declared that field as a byte sequence would send. Decoding them into
/// a string first — lossily or otherwise — would be a *string* substitution
/// wearing an image's name, and would leave the one shape this is about
/// untested.
///
/// The published types are left alone. A defect is a client that answers
/// something other than what the contract says, so the variant is built as the
/// document a client sends rather than by giving a generated type a field it
/// does not have.
///
/// # Panics
///
/// Panics when the published answer does not render back as a document, which
/// is a client no comparison could be made against.
fn the_images_bytes_in_place_of_a_field(answered: &ImageAnswer) -> Value {
    let mut variant = serde_json::to_value(answered).expect("an answer renders back as a document");
    variant["record"]["content_type"] =
        serde_json::to_value(bytes_of(answered)).expect("bytes render as a sequence of them");
    variant
}

/// The walk's own equality refuses both variants, and accepts the client.
#[test]
fn the_equality_the_walk_asserts_refuses_a_client_that_carries_the_image() {
    let root = tempfile::tempdir().expect("this walk's own root");
    let mut standing = supervisor::standing(root.path());
    let world = standing.at.clone();
    let proxy = live::Proxy::in_front_of(&world.server);
    let client = Client::new(proxy.url(), Actor::Operator);

    // A write first, so the comparison the two refusals below are about is one
    // this client has already been seen to pass on an answer of another shape.
    let written = client
        .manifest_set(
            &world.print_id,
            "a falsifying fixture is asking",
            &manifest(&world),
        )
        .expect("a manifest write is answered by a real supervisor");
    live::same("manifest_set", &written, &proxy.last().answer);

    let answered = client
        .image(&world.image_id)
        .expect("an image read is answered by a real supervisor");
    let sent = proxy.last().answer;

    // The image is the one this world opened, so what the variants below carry
    // is this print's own image rather than a file that happened to be there.
    assert_eq!(answered.record.print_id, world.print_id);
    assert_eq!(answered.record.event_id, world.event_id);

    // The published client passes it, which is what makes the two refusals
    // below about the variants rather than about the comparison.
    live::same("image", &answered, &sent);

    let with_base64 = base64_of_the_image_into_a_string_field(&answered);
    assert_ne!(
        with_base64.record.sha256, answered.record.sha256,
        "the first variant changed nothing, so it proves nothing"
    );
    assert!(
        !live::matches(&with_base64, &sent),
        "a client writing a base64 of the image into a string field was not refused"
    );

    // Not text, and not a decoding of the bytes into text: the image this world
    // stores opens `ff d8`, which no string of any encoding this system speaks
    // could carry, and what the variant puts in the field is the byte sequence
    // itself.
    let bytes = bytes_of(&answered);
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "the image this world stores is text, so a byte-valued substitution of it \
         would prove nothing a string one does not"
    );
    let with_bytes = the_images_bytes_in_place_of_a_field(&answered);
    let substituted = &with_bytes["record"]["content_type"];
    assert_eq!(
        substituted.as_array().map(Vec::len),
        Some(bytes.len()),
        "the second variant carries {substituted} where the image's own {} bytes belong",
        bytes.len()
    );
    assert!(
        !live::matches(&with_bytes, &sent),
        "a client returning the image's bytes in place of a field was not refused"
    );

    assert_eq!(
        proxy.calls(),
        2,
        "a call reached the supervisor without going through the proxy"
    );
    standing.stop();
}

/// A manifest naming the file this world's machine prints.
fn manifest(world: &supervisor::Supervisor) -> JobManifest {
    JobManifest {
        file_name: world.file_name.clone(),
        material: "PLA".to_owned(),
        nozzle_diameter_mm: 0.4,
        slicer_profile: "a falsifying fixture's own profile".to_owned(),
        allowed: BTreeMap::new(),
        metadata: BTreeMap::new(),
    }
}
