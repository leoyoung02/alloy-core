use alloy_json_abi::{ContractObject, JsonAbi};
use proc_macro2::{Ident, TokenStream};
use quote::{quote, TokenStreamExt};
use syn::{Attribute, Result};

pub fn expand(name: Ident, json: ContractObject, attrs: Vec<Attribute>) -> Result<TokenStream> {
    let ContractObject { abi, bytecode, deployed_bytecode } = json;

    let mut abi = abi.ok_or_else(|| syn::Error::new(name.span(), "ABI not found in JSON"))?;
    let sol = abi_to_sol(&name, &mut abi);
    let sol_interface_tokens = tokens_for_sol(&name, &sol)?;
    let bytecode = bytecode.map(|bytes| {
        let s = bytes.to_string();
        quote!(bytecode = #s,)
    });
    let deployed_bytecode = deployed_bytecode.map(|bytes| {
        let s = bytes.to_string();
        quote!(deployed_bytecode = #s)
    });

    let doc_str = format!(
        "\n\n\
Generated by the following Solidity interface...
```solidity
{sol}
```
...which was generated by the following JSON ABI:
```json
{json_s}
```",
        json_s = serde_json::to_string_pretty(&abi).unwrap()
    );
    let tokens = quote! {
        #(#attrs)*
        #[doc = #doc_str]
        #[sol(#bytecode #deployed_bytecode)]
        #sol_interface_tokens
    };

    let ast = syn::parse2(tokens).map_err(|e| {
        let msg = format!(
            "failed to parse ABI-generated tokens into a Solidity AST: {e}.\n\
             This is a bug. We would appreciate a bug report: \
             https://github.com/alloy-rs/core/issues/new/choose"
        );
        syn::Error::new(name.span(), msg)
    })?;
    crate::expand::expand(ast)
}

fn abi_to_sol(name: &Ident, abi: &mut JsonAbi) -> String {
    dedup_abi(abi);
    abi.to_sol(&name.to_string())
}

/// Returns `sol!` tokens.
fn tokens_for_sol(name: &Ident, sol: &str) -> Result<TokenStream> {
    let mk_err = |s: &str| {
        let msg = format!(
            "`JsonAbi::to_sol` generated invalid Rust tokens: {s}\n\
             This is a bug. We would appreciate a bug report: \
             https://github.com/alloy-rs/core/issues/new/choose"
        );
        syn::Error::new(name.span(), msg)
    };
    let brace_idx = sol.find('{').ok_or_else(|| mk_err("missing `{`"))?;
    let tts =
        syn::parse_str::<TokenStream>(&sol[brace_idx..]).map_err(|e| mk_err(&e.to_string()))?;

    let mut tokens = TokenStream::new();
    // append `name` manually for the span
    tokens.append(id("interface"));
    tokens.append(name.clone());
    tokens.extend(tts);
    Ok(tokens)
}

fn dedup_abi(abi: &mut JsonAbi) {
    macro_rules! deduper {
        () => {
            |a, b| {
                assert_eq!(a.name, b.name);
                a.inputs == b.inputs
            }
        };
    }
    for functions in abi.functions.values_mut() {
        functions.dedup_by(deduper!());
    }
    for errors in abi.errors.values_mut() {
        errors.dedup_by(deduper!());
    }
    for events in abi.events.values_mut() {
        events.dedup_by(deduper!());
    }
}

#[track_caller]
#[inline]
fn id(s: impl AsRef<str>) -> Ident {
    // Ident::new panics on rust keywords
    syn::parse_str(s.as_ref()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::Item;
    use std::path::{Path, PathBuf};

    #[test]
    #[cfg_attr(miri, ignore = "no fs")]
    fn abi() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../json-abi/tests/abi");
        for file in std::fs::read_dir(path).unwrap() {
            let path = file.unwrap().path();
            assert_eq!(path.extension(), Some("json".as_ref()));
            if path.file_name() == Some("LargeFunction.json".as_ref()) {
                continue;
            }
            parse_test(&std::fs::read_to_string(&path).unwrap(), path.to_str().unwrap());
        }
    }

    #[allow(clippy::single_match)]
    fn parse_test(s: &str, path: &str) {
        let (c, name) = expand_test(s, path);
        match name {
            "Udvts" => {
                assert_eq!(c.name, "Udvts");
                assert_eq!(c.body.len(), 12, "{}, {:#?}", c.body.len(), c);
                let [Item::Udt(a), Item::Udt(b), Item::Udt(c), rest @ ..] = &c.body[..] else {
                    for item in &c.body {
                        eprintln!("{item:?}\n");
                    }
                    panic!();
                };

                assert_eq!(a.name, "ItemType");
                assert_eq!(a.ty.to_string(), "bytes32");

                assert_eq!(b.name, "OrderType");
                assert_eq!(b.ty.to_string(), "uint256");

                assert_eq!(c.name, "Side");
                assert_eq!(c.ty.to_string(), "bool");

                rest[..8].iter().for_each(|item| assert!(matches!(item, Item::Struct(_))));

                let last = &rest[8];
                assert!(rest[9..].is_empty());
                let Item::Function(f) = last else { panic!("{last:#?}") };
                assert_eq!(f.name.as_ref().unwrap(), "fulfillAvailableAdvancedOrders");
                assert!(f.attributes.contains(&ast::FunctionAttribute::Mutability(
                    ast::Mutability::Payable(Default::default())
                )));
                assert!(f.attributes.contains(&ast::FunctionAttribute::Visibility(
                    ast::Visibility::External(Default::default())
                )));

                let args = &f.arguments;
                assert_eq!(args.len(), 7);

                assert_eq!(args[0].ty.to_string(), "AdvancedOrder[]");
                assert_eq!(args[0].name.as_ref().unwrap(), "a");
                assert_eq!(args[1].ty.to_string(), "CriteriaResolver[]");
                assert_eq!(args[1].name.as_ref().unwrap(), "b");
                assert_eq!(args[2].ty.to_string(), "FulfillmentComponent[][]");
                assert_eq!(args[2].name.as_ref().unwrap(), "c");
                assert_eq!(args[3].ty.to_string(), "FulfillmentComponent[][]");
                assert_eq!(args[3].name.as_ref().unwrap(), "d");
                assert_eq!(args[4].ty.to_string(), "bytes32");
                assert_eq!(args[4].name.as_ref().unwrap(), "fulfillerConduitKey");
                assert_eq!(args[5].ty.to_string(), "address");
                assert_eq!(args[5].name.as_ref().unwrap(), "recipient");
                assert_eq!(args[6].ty.to_string(), "uint256");
                assert_eq!(args[6].name.as_ref().unwrap(), "maximumFulfilled");

                let returns = &f.returns.as_ref().unwrap().returns;
                assert_eq!(returns.len(), 2);

                assert_eq!(returns[0].ty.to_string(), "bool[]");
                assert_eq!(returns[0].name.as_ref().unwrap(), "e");
                assert_eq!(returns[1].ty.to_string(), "Execution[]");
                assert_eq!(returns[1].name.as_ref().unwrap(), "f");
            }
            "EnumsInLibraryFunctions" => {
                assert_eq!(c.name, "EnumsInLibraryFunctions");
                assert_eq!(c.body.len(), 5);
                let [Item::Udt(the_enum), Item::Function(f_array), Item::Function(f_arrays), Item::Function(f_dyn_array), Item::Function(f_just_enum)] =
                    &c.body[..]
                else {
                    panic!("{c:#?}");
                };

                assert_eq!(the_enum.name, "TheEnum");
                assert_eq!(the_enum.ty.to_string(), "uint8");

                let function_tests = [
                    (f_array, "enumArray", "TheEnum[2]"),
                    (f_arrays, "enumArrays", "TheEnum[][69][]"),
                    (f_dyn_array, "enumDynArray", "TheEnum[]"),
                    (f_just_enum, "enum_", "TheEnum"),
                ];
                for (f, name, ty) in function_tests {
                    assert_eq!(f.name.as_ref().unwrap(), name);
                    assert_eq!(f.arguments.type_strings().collect::<Vec<_>>(), [ty]);
                    let ret = &f.returns.as_ref().expect("no returns").returns;
                    assert_eq!(ret.type_strings().collect::<Vec<_>>(), [ty]);
                }
            }
            _ => {}
        }
    }

    fn expand_test<'a>(s: &str, path: &'a str) -> (ast::ItemContract, &'a str) {
        let mut abi: JsonAbi = serde_json::from_str(s).unwrap();
        let name = Path::new(path).file_stem().unwrap().to_str().unwrap();

        let name_id = id(name);
        let sol = abi_to_sol(&name_id, &mut abi);
        let tokens = match tokens_for_sol(&name_id, &sol) {
            Ok(tokens) => tokens,
            Err(e) => {
                let path = write_tmp_sol(name, &sol);
                panic!(
                    "couldn't expand JSON ABI for {name:?}: {e}\n\
                     emitted interface: {}",
                    path.display()
                );
            }
        };

        let ast = match syn::parse2::<ast::File>(tokens.clone()) {
            Ok(ast) => ast,
            Err(e) => {
                let spath = write_tmp_sol(name, &sol);
                let tpath = write_tmp_sol(&format!("{name}.tokens"), &tokens.to_string());
                panic!(
                    "couldn't parse expanded JSON ABI back to AST for {name:?}: {e}\n\
                     emitted interface: {}\n\
                     emitted tokens:    {}",
                    spath.display(),
                    tpath.display(),
                )
            }
        };

        let mut items = ast.items.into_iter();
        let Some(Item::Contract(c)) = items.next() else {
            panic!("first item is not a contract");
        };
        let next = items.next();
        assert!(next.is_none(), "AST does not contain exactly one item: {next:#?}, {items:#?}");
        assert!(!c.body.is_empty(), "generated contract is empty");
        (c, name)
    }

    fn write_tmp_sol(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("sol-macro-{name}.sol"));
        std::fs::write(&path, contents).unwrap();
        let _ = std::process::Command::new("forge").arg("fmt").arg(&path).output();
        path
    }
}
