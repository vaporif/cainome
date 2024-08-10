use anyhow::Result;
use cainome_parser::tokens::StateMutability;
use cainome_parser::{AbiParser, TokenizedAbi};
use camino::Utf8PathBuf;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;

mod execution_version;
mod expand;
pub use execution_version::{ExecutionVersion, ParseExecutionVersionError};

use crate::expand::utils;
pub use crate::expand::{CairoContract, CairoEnum, CairoEnumEvent, CairoFunction, CairoStruct};

///Type-safe contract bindings generated by Abigen.
#[derive(Clone)]
pub struct ContractBindings {
    /// Name of the contract.
    pub name: String,
    /// Tokenized ABI written to a `[TokenStream2]`.
    pub tokens: TokenStream2,
}

impl ContractBindings {
    /// Writes the bindings to the specified file.
    ///
    /// # Arguments
    ///
    /// * `file` - The path to the file to write the bindings to.
    pub fn write_to_file(&self, file: &str) -> io::Result<()> {
        fs::write(file, self.to_string())
    }
}

impl fmt::Display for ContractBindings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let syntax_tree = syn::parse2::<syn::File>(self.tokens.clone()).unwrap();
        let s = prettyplease::unparse(&syntax_tree);
        f.write_str(&s)
    }
}

impl fmt::Debug for ContractBindings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContractBindings")
            .field("name", &self.name)
            .finish()
    }
}

/// Programmatically generate type-safe Rust bindings for an Starknet smart contract from its ABI.
///
/// Currently only one contract at a time is supported.
#[derive(Debug, Clone)]
pub struct Abigen {
    /// Name of the contract, used as the variable name in the generated code
    /// to identify the contract.
    pub contract_name: String,
    /// The path to a sierra artifact or a JSON with ABI entries only.
    pub abi_source: Utf8PathBuf,
    /// Types aliases to avoid name conflicts, as for now the types are limited to the
    /// latest segment of the fully qualified path.
    pub types_aliases: HashMap<String, String>,
    /// The version of transaction to be executed.
    pub execution_version: ExecutionVersion,
}

impl Abigen {
    /// Creates a new instance of `Abigen`.
    ///
    /// # Arguments
    ///
    /// * `contract_name` - Name of the contract, used as the variable name in the generated code
    ///   to identify the contract.
    /// * `abi_source` - The path to a sierra artifact or a JSON with ABI entries only.
    pub fn new(contract_name: &str, abi_source: &str) -> Self {
        Self {
            contract_name: contract_name.to_string(),
            abi_source: Utf8PathBuf::from(abi_source),
            types_aliases: HashMap::new(),
            execution_version: ExecutionVersion::V1,
        }
    }

    /// Sets the types aliases to avoid name conflicts.
    ///
    /// # Arguments
    ///
    /// * `types_aliases` - Types aliases to avoid name conflicts.
    pub fn with_types_aliases(mut self, types_aliases: HashMap<String, String>) -> Self {
        self.types_aliases = types_aliases;
        self
    }

    /// Sets the execution version to be used.
    ///
    /// # Arguments
    ///
    /// * `execution_version` - The version of transaction to be executed.
    pub fn with_execution_version(mut self, execution_version: ExecutionVersion) -> Self {
        self.execution_version = execution_version;
        self
    }

    /// Generates the contract bindings.
    pub fn generate(&self) -> Result<ContractBindings> {
        let file_content = std::fs::read_to_string(&self.abi_source)?;

        match AbiParser::tokens_from_abi_string(&file_content, &self.types_aliases) {
            Ok(tokens) => {
                let expanded =
                    abi_to_tokenstream(&self.contract_name, &tokens, self.execution_version);

                Ok(ContractBindings {
                    name: self.contract_name.clone(),
                    tokens: expanded,
                })
            }
            Err(e) => {
                anyhow::bail!(
                    "Abi source {} could not be parsed {:?}. ABI file should be a JSON with an array of abi entries or a Sierra artifact.",
                    self.abi_source, e
                )
            }
        }
    }
}

/// Converts the given ABI (in it's tokenize form) into rust bindings.
///
/// # Arguments
///
/// * `contract_name` - Name of the contract.
/// * `abi_tokens` - Tokenized ABI.
pub fn abi_to_tokenstream(
    contract_name: &str,
    abi_tokens: &TokenizedAbi,
    execution_version: ExecutionVersion,
) -> TokenStream2 {
    let contract_name = utils::str_to_ident(contract_name);

    let mut tokens: Vec<TokenStream2> = vec![];

    tokens.push(CairoContract::expand(contract_name.clone()));

    for s in &abi_tokens.structs {
        let s_composite = s.to_composite().expect("composite expected");
        tokens.push(CairoStruct::expand_decl(s_composite));
        tokens.push(CairoStruct::expand_impl(s_composite));
    }

    for e in &abi_tokens.enums {
        let e_composite = e.to_composite().expect("composite expected");
        tokens.push(CairoEnum::expand_decl(e_composite));
        tokens.push(CairoEnum::expand_impl(e_composite));

        tokens.push(CairoEnumEvent::expand(
            e.to_composite().expect("composite expected"),
            &abi_tokens.enums,
            &abi_tokens.structs,
        ));
    }

    let mut reader_views = vec![];
    let mut views = vec![];
    let mut externals = vec![];

    // Interfaces are not yet reflected in the generated contract.
    // Then, the standalone functions and functions from interfaces are put together.
    let mut functions = abi_tokens.functions.clone();
    for funcs in abi_tokens.interfaces.values() {
        functions.extend(funcs.clone());
    }

    for f in functions {
        let f = f.to_function().expect("function expected");
        match f.state_mutability {
            StateMutability::View => {
                reader_views.push(CairoFunction::expand(f, true, execution_version));
                views.push(CairoFunction::expand(f, false, execution_version));
            }
            StateMutability::External => {
                externals.push(CairoFunction::expand(f, false, execution_version))
            }
        }
    }

    let reader = utils::str_to_ident(format!("{}Reader", contract_name).as_str());

    tokens.push(quote! {
        impl<A: starknet::accounts::ConnectedAccount + Sync> #contract_name<A> {
            #(#views)*
            #(#externals)*
        }

        impl<P: starknet::providers::Provider + Sync> #reader<P> {
            #(#reader_views)*
        }
    });

    let expanded = quote! {
        #(#tokens)*
    };

    expanded
}
