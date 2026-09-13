//! Every schema this crate generates admits the values its own type emits.

use printobserver_types::contract::declared;

/// Every declared type's schema admits its own canonical values.
///
/// The schemas are generated from the types, so a value one type emits is a
/// value its own schema admits; a schema that refused it would be a schema
/// transcribed rather than generated.
#[test]
fn every_generated_schema_admits_its_own_canonical_values() {
    for entry in declared() {
        let schema = entry.schema();
        let validator = jsonschema::validator_for(&schema)
            .unwrap_or_else(|error| panic!("{}'s schema does not compile: {error}", entry.name));
        for value in entry.samples() {
            assert!(
                validator.is_valid(&value),
                "{}'s schema refused {value}",
                entry.name
            );
        }
    }
}
