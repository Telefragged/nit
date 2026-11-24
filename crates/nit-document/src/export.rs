use ts_rs::{Config, TS};

#[test]
fn write_wire_types() {
    let Some(path) = std::env::var_os("TYPES_GEN_OUT") else {
        return;
    };
    let config = Config::from_env();
    let mut out = String::from(
        "// @generated from crates/nit-document by `nix run .#gen-types` — DO NOT EDIT.\n\
         // Change the Rust types, then regenerate.\n\n\
         import type {\n  AttachmentHash,\n  ChangeNumber,\n  RevisionNumber,\n  Side,\n  Tags,\n} from \"./types.gen\";\n\n",
    );
    macro_rules! emit {
        ($($t:ty),* $(,)?) => {$({
            if let Some(docs) = <$t as TS>::docs() {
                out.push_str(&docs);
            }
            out.push_str("export ");
            out.push_str(&<$t as TS>::decl(&config));
            out.push_str("\n\n");
        })*};
    }
    emit!(
        crate::Writer,
        crate::Reference,
        crate::LineRange,
        crate::PlacedReference,
        crate::Version,
        crate::wire::DocumentNumber,
        crate::wire::CreateDocument,
        crate::wire::DocumentSummary,
        crate::wire::DocumentList,
        crate::wire::DocumentQuery,
        crate::wire::DocumentSubscription,
        crate::wire::DocumentSent,
    );
    std::fs::write(path, out).expect("write document.gen.ts");
}
