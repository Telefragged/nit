//! The `OpenAPI` document that the routes' utoipa annotations produce.

#[test]
fn every_schema_reference_resolves() {
    let openapi = nit::api::openapi();
    let schemas = openapi.components.as_ref().map(|c| &c.schemas);
    let json = openapi.to_json().expect("the OpenAPI document serializes");
    for reference in json.split("\"#/components/schemas/").skip(1) {
        let name = &reference[..reference.find('"').expect("a $ref ends in a quote")];
        assert!(
            schemas.is_some_and(|s| s.contains_key(name)),
            "no schema {name}: register it in ApiDoc"
        );
    }
}
