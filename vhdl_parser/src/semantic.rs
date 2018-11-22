// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2018, Olof Kraigher olof.kraigher@gmail.com

use ast::*;
use latin_1::Latin1String;
use library::DesignRoot;
use message::{Message, MessageHandler};
use source::WithPos;
use symbol_table::{Symbol, SymbolTable};

extern crate fnv;
use self::fnv::FnvHashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

#[derive(Clone)]
struct DeclarativeItem {
    designator: WithPos<Designator>,
    may_overload: bool,
    is_deferred: bool,
}

impl DeclarativeItem {
    fn new(designator: impl Into<WithPos<Designator>>) -> DeclarativeItem {
        DeclarativeItem {
            designator: designator.into(),
            may_overload: false,
            is_deferred: false,
        }
    }
    fn from_ident(ident: &Ident) -> DeclarativeItem {
        DeclarativeItem::new(ident.to_owned().map_into(Designator::Identifier))
    }

    fn with_overload(mut self, value: bool) -> DeclarativeItem {
        self.may_overload = value;
        self
    }

    fn with_deferred(mut self, value: bool) -> DeclarativeItem {
        self.is_deferred = value;
        self
    }
}

#[derive(Clone)]
struct DeclarativeRegion {
    decls: FnvHashMap<Designator, DeclarativeItem>,
}

impl DeclarativeRegion {
    fn new() -> DeclarativeRegion {
        DeclarativeRegion {
            decls: FnvHashMap::default(),
        }
    }

    fn add(&mut self, decl: DeclarativeItem, messages: &mut MessageHandler) {
        match self.decls.entry(decl.designator.item.clone()) {
            Entry::Occupied(mut entry) => {
                let old_decl = entry.get_mut();

                if !decl.may_overload || !old_decl.may_overload {
                    if !decl.is_deferred && old_decl.is_deferred {
                        std::mem::replace(old_decl, decl);
                    } else {
                        let msg = Message::error(
                            &decl.designator,
                            format!("Duplicate declaration of '{}'", decl.designator.item),
                        ).related(&old_decl.designator, "Previously defined here");
                        messages.push(msg)
                    }
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(decl);
            }
        }
    }

    fn add_interface_list(
        &mut self,
        declarations: &[InterfaceDeclaration],
        messages: &mut MessageHandler,
    ) {
        for decl in declarations.iter() {
            for item in decl.declarative_items() {
                self.add(item, messages);
            }
        }
    }

    fn add_declarative_part(
        &mut self,
        declarations: &[Declaration],
        messages: &mut MessageHandler,
    ) {
        for decl in declarations.iter() {
            for item in decl.declarative_items() {
                self.add(item, messages);
            }
        }
    }

    fn add_element_declarations(
        &mut self,
        declarations: &[ElementDeclaration],
        messages: &mut MessageHandler,
    ) {
        for decl in declarations.iter() {
            self.add(DeclarativeItem::from_ident(&decl.ident), messages);
        }
    }
}

impl SubprogramDesignator {
    fn to_designator(self) -> Designator {
        match self {
            SubprogramDesignator::Identifier(ident) => Designator::Identifier(ident),
            SubprogramDesignator::OperatorSymbol(ident) => Designator::OperatorSymbol(ident),
        }
    }
}

impl SubprogramDeclaration {
    fn designator(&self) -> WithPos<Designator> {
        match self {
            SubprogramDeclaration::Function(ref function) => function
                .designator
                .clone()
                .map_into(|des| des.to_designator()),
            SubprogramDeclaration::Procedure(ref procedure) => procedure
                .designator
                .clone()
                .map_into(|des| des.to_designator()),
        }
    }
}

impl EnumerationLiteral {
    fn to_designator(self) -> Designator {
        match self {
            EnumerationLiteral::Identifier(ident) => Designator::Identifier(ident),
            EnumerationLiteral::Character(byte) => Designator::Character(byte),
        }
    }
}

impl Declaration {
    fn declarative_items(&self) -> Vec<DeclarativeItem> {
        match self {
            Declaration::Alias(alias) => vec![
                DeclarativeItem::new(alias.designator.clone())
                    .with_overload(alias.signature.is_some()),
            ],
            Declaration::Object(ObjectDeclaration {
                ref ident,
                ref class,
                ref expression,
                ..
            }) => vec![
                DeclarativeItem::from_ident(ident)
                    .with_deferred(*class == ObjectClass::Constant && expression.is_none()),
            ],
            Declaration::File(FileDeclaration { ref ident, .. }) => {
                vec![DeclarativeItem::from_ident(ident)]
            }
            Declaration::Component(ComponentDeclaration { ref ident, .. }) => {
                vec![DeclarativeItem::from_ident(ident)]
            }
            Declaration::Attribute(ref attr) => match attr {
                Attribute::Declaration(AttributeDeclaration { ref ident, .. }) => {
                    vec![DeclarativeItem::from_ident(ident)]
                }
                // @TODO Ignored for now
                Attribute::Specification(..) => vec![],
            },
            Declaration::SubprogramBody(body) => {
                vec![DeclarativeItem::new(body.specification.designator()).with_overload(true)]
            }
            Declaration::SubprogramDeclaration(decl) => {
                vec![DeclarativeItem::new(decl.designator()).with_overload(true)]
            }
            // @TODO Ignored for now
            Declaration::Use(..) => vec![],
            Declaration::Package(ref package) => vec![DeclarativeItem::from_ident(&package.ident)],
            Declaration::Configuration(..) => vec![],
            Declaration::Type(TypeDeclaration {
                def: TypeDefinition::ProtectedBody(..),
                ..
            }) => vec![],
            Declaration::Type(TypeDeclaration {
                def: TypeDefinition::Incomplete,
                ..
            }) => vec![],
            Declaration::Type(TypeDeclaration {
                ref ident,
                def: TypeDefinition::Enumeration(ref enumeration),
            }) => {
                let mut items = vec![DeclarativeItem::from_ident(ident)];
                for literal in enumeration.iter() {
                    items.push(
                        DeclarativeItem::new(literal.clone().map_into(|lit| lit.to_designator()))
                            .with_overload(true),
                    )
                }
                items
            }
            Declaration::Type(TypeDeclaration { ref ident, .. }) => {
                vec![DeclarativeItem::from_ident(ident)]
            }
        }
    }
}

impl InterfaceDeclaration {
    fn declarative_items(&self) -> Vec<DeclarativeItem> {
        match self {
            InterfaceDeclaration::File(InterfaceFileDeclaration { ref ident, .. }) => {
                vec![DeclarativeItem::from_ident(ident)]
            }
            InterfaceDeclaration::Object(InterfaceObjectDeclaration { ref ident, .. }) => {
                vec![DeclarativeItem::from_ident(ident)]
            }
            InterfaceDeclaration::Type(ref ident) => vec![DeclarativeItem::from_ident(ident)],
            InterfaceDeclaration::Subprogram(decl, ..) => {
                vec![DeclarativeItem::new(decl.designator()).with_overload(true)]
            }
            InterfaceDeclaration::Package(ref package) => {
                vec![DeclarativeItem::from_ident(&package.ident)]
            }
        }
    }
}

impl std::fmt::Display for Designator {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Designator::Identifier(ref sym) => write!(f, "{}", sym),
            Designator::OperatorSymbol(ref latin1) => write!(f, "\"{}\"", latin1),
            Designator::Character(byte) => write!(f, "'{}'", *byte as char),
        }
    }
}

/// Check that no homographs are defined in the element declarations
fn check_element_declaration_unique_ident(
    declarations: &[ElementDeclaration],
    messages: &mut MessageHandler,
) {
    DeclarativeRegion::new().add_element_declarations(declarations, messages);
}

/// Check that no homographs are defined in the interface list
fn check_interface_list_unique_ident(
    declarations: &[InterfaceDeclaration],
    messages: &mut MessageHandler,
) {
    DeclarativeRegion::new().add_interface_list(declarations, messages);
}

impl SubprogramDeclaration {
    fn interface_list<'a>(&'a self) -> &[InterfaceDeclaration] {
        match self {
            SubprogramDeclaration::Function(fun) => &fun.parameter_list,
            SubprogramDeclaration::Procedure(proc) => &proc.parameter_list,
        }
    }
}
fn check_declarative_part_unique_ident(
    declarations: &[Declaration],
    messages: &mut MessageHandler,
) {
    let mut region = DeclarativeRegion::new();
    region.add_declarative_part(declarations, messages);
    check_declarative_part_unique_ident_inner(declarations, messages);
}

/// Check that no homographs are defined in the declarative region
fn check_declarative_part_unique_ident_inner(
    declarations: &[Declaration],
    messages: &mut MessageHandler,
) {
    for decl in declarations.iter() {
        match decl {
            Declaration::Component(ref component) => {
                check_interface_list_unique_ident(&component.generic_list, messages);
                check_interface_list_unique_ident(&component.port_list, messages);
            }
            Declaration::SubprogramBody(ref body) => {
                check_interface_list_unique_ident(body.specification.interface_list(), messages);
                check_declarative_part_unique_ident(&body.declarations, messages);
            }
            Declaration::SubprogramDeclaration(decl) => {
                check_interface_list_unique_ident(decl.interface_list(), messages);
            }
            Declaration::Type(type_decl) => match type_decl.def {
                TypeDefinition::ProtectedBody(ref body) => {
                    check_declarative_part_unique_ident(&body.decl, messages);
                }
                TypeDefinition::Protected(ref prot_decl) => {
                    for item in prot_decl.items.iter() {
                        match item {
                            ProtectedTypeDeclarativeItem::Subprogram(subprogram) => {
                                check_interface_list_unique_ident(
                                    subprogram.interface_list(),
                                    messages,
                                );
                            }
                        }
                    }
                }
                TypeDefinition::Record(ref decls) => {
                    check_element_declaration_unique_ident(decls, messages);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn check_generate_body(body: &GenerateBody, messages: &mut MessageHandler) {
    if let Some(ref decl) = body.decl {
        check_declarative_part_unique_ident(&decl, messages);
    }
    check_concurrent_part(&body.statements, messages);
}

fn check_concurrent_statement(
    statement: &LabeledConcurrentStatement,
    messages: &mut MessageHandler,
) {
    match statement.statement {
        ConcurrentStatement::Block(ref block) => {
            check_declarative_part_unique_ident(&block.decl, messages);
            check_concurrent_part(&block.statements, messages);
        }
        ConcurrentStatement::Process(ref process) => {
            check_declarative_part_unique_ident(&process.decl, messages);
        }
        ConcurrentStatement::ForGenerate(ref gen) => {
            check_generate_body(&gen.body, messages);
        }
        ConcurrentStatement::IfGenerate(ref gen) => {
            for conditional in gen.conditionals.iter() {
                check_generate_body(&conditional.item, messages);
            }
            if let Some(ref else_item) = gen.else_item {
                check_generate_body(else_item, messages);
            }
        }
        ConcurrentStatement::CaseGenerate(ref gen) => {
            for alternative in gen.alternatives.iter() {
                check_generate_body(&alternative.item, messages);
            }
        }
        _ => {}
    }
}

fn check_concurrent_part(statements: &[LabeledConcurrentStatement], messages: &mut MessageHandler) {
    for statement in statements.iter() {
        check_concurrent_statement(statement, messages);
    }
}

fn check_package_declaration(
    package: &PackageDeclaration,
    messages: &mut MessageHandler,
) -> DeclarativeRegion {
    let mut region = DeclarativeRegion::new();
    if let Some(ref list) = package.generic_clause {
        region.add_interface_list(list, messages);
    }
    region.add_declarative_part(&package.decl, messages);
    check_declarative_part_unique_ident_inner(&package.decl, messages);
    region
}

fn check_architecture_body(
    entity_region: &mut DeclarativeRegion,
    architecture: &ArchitectureBody,
    messages: &mut MessageHandler,
) {
    entity_region.add_declarative_part(&architecture.decl, messages);
    check_declarative_part_unique_ident_inner(&architecture.decl, messages);
    check_concurrent_part(&architecture.statements, messages);
}

fn check_package_body(
    package_region: &mut DeclarativeRegion,
    package: &PackageBody,
    messages: &mut MessageHandler,
) {
    package_region.add_declarative_part(&package.decl, messages);
    check_declarative_part_unique_ident_inner(&package.decl, messages);
}

fn check_entity_declaration(
    entity: &EntityDeclaration,
    messages: &mut MessageHandler,
) -> DeclarativeRegion {
    let mut region = DeclarativeRegion::new();
    if let Some(ref list) = entity.generic_clause {
        region.add_interface_list(list, messages);
    }
    if let Some(ref list) = entity.port_clause {
        region.add_interface_list(list, messages);
    }
    region.add_declarative_part(&entity.decl, messages);
    check_concurrent_part(&entity.statements, messages);

    region
}

pub struct Analyzer {
    work_sym: Symbol,
    std_sym: Symbol,
}

impl Analyzer {
    pub fn new(symtab: Arc<SymbolTable>) -> Analyzer {
        Analyzer {
            work_sym: symtab.insert(&Latin1String::new(b"work")),
            std_sym: symtab.insert(&Latin1String::new(b"std")),
        }
    }

    fn check_context_clause(
        &self,
        root: &DesignRoot,
        context_clause: &Vec<WithPos<ContextItem>>,
        messages: &mut MessageHandler,
    ) {
        for context_item in context_clause.iter() {
            match context_item.item {
                ContextItem::Library(LibraryClause { ref name_list }) => {
                    for library_name in name_list.iter() {
                        if self.std_sym == library_name.item {
                            // std is pre-defined
                        } else if self.work_sym == library_name.item {
                            messages.push(Message::hint(
                                &library_name,
                                format!("Library clause not necessary for current working library"),
                            ))
                        } else if !root.has_library(&library_name.item) {
                            messages.push(Message::error(
                                &library_name,
                                format!("No such library '{}'", library_name.item),
                            ))
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn analyze(&self, root: &DesignRoot, messages: &mut MessageHandler) {
        for library in root.iter_libraries() {
            for entity in library.entities() {
                self.check_context_clause(root, &entity.entity.context_clause, messages);
                let mut region = check_entity_declaration(&entity.entity.unit, messages);
                for architecture in entity.architectures.values() {
                    self.check_context_clause(root, &architecture.context_clause, messages);
                    check_architecture_body(&mut region.clone(), &architecture.unit, messages);
                }
            }

            for package in library.packages() {
                self.check_context_clause(root, &package.package.context_clause, messages);
                let mut region = check_package_declaration(&package.package.unit, messages);
                if let Some(ref body) = package.body {
                    self.check_context_clause(root, &body.context_clause, messages);
                    check_package_body(&mut region.clone(), &body.unit, messages);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::Library;
    use message::Message;
    use test_util::{check_messages, check_no_messages, Code, CodeBuilder};

    fn expected_message(code: &Code, name: &str, occ1: usize, occ2: usize) -> Message {
        Message::error(
            code.s(&name, occ2),
            format!("Duplicate declaration of '{}'", &name),
        ).related(code.s(&name, occ1), "Previously defined here")
    }

    fn expected_messages(code: &Code, names: &[&str]) -> Vec<Message> {
        let mut messages = Vec::new();
        for name in names {
            messages.push(expected_message(code, name, 1, 2));
        }
        messages
    }

    fn expected_messages_multi(code1: &Code, code2: &Code, names: &[&str]) -> Vec<Message> {
        let mut messages = Vec::new();
        for name in names {
            messages.push(
                Message::error(
                    code2.s1(&name),
                    format!("Duplicate declaration of '{}'", &name),
                ).related(code1.s1(&name), "Previously defined here"),
            )
        }
        messages
    }

    #[test]
    fn allows_unique_names() {
        let code = Code::new(
            "
constant a : natural := 0;
constant b : natural := 0;
constant c : natural := 0;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_no_messages(&messages);
    }

    #[test]
    fn allows_deferred_constant() {
        let code = Code::new(
            "
constant a : natural;
constant a : natural := 0;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_no_messages(&messages);
    }

    #[test]
    fn forbid_deferred_constant_after_constant() {
        let code = Code::new(
            "
constant a1 : natural := 0;
constant a1 : natural;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_multiple_constant_after_deferred_constant() {
        let code = Code::new(
            "
constant a1 : natural;
constant a1 : natural := 0;
constant a1 : natural := 0;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, vec![expected_message(&code, "a1", 2, 3)]);
    }

    #[test]
    fn forbid_homographs() {
        let code = Code::new(
            "
constant a1 : natural := 0;
constant a : natural := 0;
constant a1 : natural := 0;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn allows_protected_type_and_body_with_same_name() {
        let code = Code::new(
            "
type prot_t is protected
end protected;

type prot_t is protected body
end protected body;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_no_messages(&messages);
    }

    #[test]
    fn allows_incomplete_type_definition() {
        let code = Code::new(
            "
type rec_t;
type rec_t is record
end record;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_no_messages(&messages);
    }

    #[test]
    fn forbid_homographs_in_subprogram_bodies() {
        let code = Code::new(
            "
procedure proc(a1, a, a1 : natural) is
  constant b1 : natural := 0;
  constant b : natural := 0;
  constant b1 : natural := 0;

  procedure nested_proc(c1, c, c1 : natural) is
    constant d1 : natural := 0;
    constant d : natural := 0;
    constant d1 : natural := 0;
  begin
  end;

begin
end;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(
            messages,
            expected_messages(&code, &["a1", "b1", "c1", "d1"]),
        );
    }

    #[test]
    fn forbid_homographs_in_component_declarations() {
        let code = Code::new(
            "
component comp is
  generic (
    a1 : natural;
    a : natural;
    a1 : natural
  );
  port (
    b1 : natural;
    b : natural;
    b1 : natural
  );
end component;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1", "b1"]));
    }

    #[test]
    fn forbid_homographs_in_record_type_declarations() {
        let code = Code::new(
            "
type rec_t is record
  a1 : natural;
  a : natural;
  a1 : natural;
end record;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_in_proteced_type_declarations() {
        let code = Code::new(
            "
type prot_t is protected
  procedure proc(a1, a, a1 : natural);
end protected;

type prot_t is protected body
  constant b1 : natural := 0;
  constant b : natural := 0;
  constant b1 : natural := 0;
end protected body;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1", "b1"]));
    }

    #[test]
    fn forbid_homographs_in_subprogram_declarations() {
        let code = Code::new(
            "
procedure proc(a1, a, a1 : natural);
function fun(b1, a, b1 : natural) return natural;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1", "b1"]));
    }

    #[test]
    fn forbid_homographs_in_block() {
        let code = Code::new(
            "
blk : block
  constant a1 : natural := 0;
  constant a : natural := 0;
  constant a1 : natural := 0;
begin
  process
    constant b1 : natural := 0;
    constant b : natural := 0;
    constant b1 : natural := 0;
  begin
  end process;
end block;
",
        );

        let mut messages = Vec::new();
        check_concurrent_statement(&code.concurrent_statement(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1", "b1"]));
    }

    #[test]
    fn forbid_homographs_in_process() {
        let code = Code::new(
            "
process
  constant a1 : natural := 0;
  constant a : natural := 0;
  constant a1 : natural := 0;
begin
end process;
",
        );

        let mut messages = Vec::new();
        check_concurrent_statement(&code.concurrent_statement(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_for_generate() {
        let code = Code::new(
            "
gen_for: for i in 0 to 3 generate
  constant a1 : natural := 0;
  constant a : natural := 0;
  constant a1 : natural := 0;
begin
  process
    constant b1 : natural := 0;
    constant b : natural := 0;
    constant b1 : natural := 0;
  begin
  end process;
end generate;
",
        );

        let mut messages = Vec::new();
        check_concurrent_statement(&code.concurrent_statement(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1", "b1"]));
    }

    #[test]
    fn forbid_homographs_if_generate() {
        let code = Code::new(
            "
gen_if: if true generate
  constant a1 : natural := 0;
  constant a : natural := 0;
  constant a1 : natural := 0;
begin

  prcss : process
    constant b1 : natural := 0;
    constant b : natural := 0;
    constant b1 : natural := 0;
  begin
  end process;

else generate
  constant c1 : natural := 0;
  constant c: natural := 0;
  constant c1 : natural := 0;
begin
  prcss : process
    constant d1 : natural := 0;
    constant d : natural := 0;
    constant d1 : natural := 0;
  begin
  end process;
end generate;
",
        );

        let mut messages = Vec::new();
        check_concurrent_statement(&code.concurrent_statement(), &mut messages);
        check_messages(
            messages,
            expected_messages(&code, &["a1", "b1", "c1", "d1"]),
        );
    }

    #[test]
    fn forbid_homographs_case_generate() {
        let code = Code::new(
            "
gen_case: case 0 generate
  when others =>
    constant a1 : natural := 0;
    constant a : natural := 0;
    constant a1 : natural := 0;
  begin
    process
      constant b1 : natural := 0;
      constant b : natural := 0;
      constant b1 : natural := 0;
    begin
    end process;
end generate;
",
        );

        let mut messages = Vec::new();
        check_concurrent_statement(&code.concurrent_statement(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1", "b1"]));
    }

    #[test]
    fn forbid_homographs_in_entity_declarations() {
        let code = Code::new(
            "
entity ent is
  generic (
    a1 : natural;
    a : natural;
    a1 : natural
  );
  port (
    b1 : natural;
    b : natural;
    b1 : natural
  );
  constant c1 : natural := 0;
  constant c : natural := 0;
  constant c1 : natural := 0;
begin

  blk : block
    constant d1 : natural := 0;
    constant d : natural := 0;
    constant d1 : natural := 0;
  begin

  end block;

end entity;
",
        );

        let mut messages = Vec::new();
        check_entity_declaration(&code.entity(), &mut messages);
        check_messages(
            messages,
            expected_messages(&code, &["a1", "b1", "c1", "d1"]),
        );
    }

    #[test]
    fn forbid_homographs_in_architecture_bodies() {
        let code = Code::new(
            "
architecture arch of ent is
  constant a1 : natural := 0;
  constant a : natural := 0;
  constant a1 : natural := 0;
begin

  blk : block
    constant b1 : natural := 0;
    constant b : natural := 0;
    constant b1 : natural := 0;
  begin
  end block;

end architecture;
",
        );

        let mut messages = Vec::new();
        check_architecture_body(
            &mut DeclarativeRegion::new(),
            &code.architecture(),
            &mut messages,
        );
        check_messages(messages, expected_messages(&code, &["a1", "b1"]));
    }

    #[test]
    fn forbid_homographs_of_type_declarations() {
        let code = Code::new(
            "
constant a1 : natural := 0;
type a1 is (foo, bar);
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_of_component_declarations() {
        let code = Code::new(
            "
constant a1 : natural := 0;
component a1 is
  port (clk : bit);
end component;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_of_file_declarations() {
        let code = Code::new(
            "
constant a1 : natural := 0;
file a1 : text;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_in_package_declarations() {
        let code = Code::new(
            "
package a1 is new pkg generic map (foo => bar);
package a1 is new pkg generic map (foo => bar);
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_in_attribute_declarations() {
        let code = Code::new(
            "
attribute a1 : string;
attribute a1 : string;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_in_alias_declarations() {
        let code = Code::new(
            "
alias a1 is foo;
alias a1 is bar;

-- Legal since subprograms are overloaded
alias b1 is foo[return natural];
alias b1 is bar[return boolean];
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_for_overloaded_vs_non_overloaded() {
        let code = Code::new(
            "
alias a1 is foo;
alias a1 is bar[return boolean];

function b1 return natural;
constant b1 : natural := 0;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1", "b1"]));
    }

    #[test]
    fn enum_literals_may_overload() {
        let code = Code::new(
            "
type enum_t is (a1, b1);

-- Ok since enumerations may overload
type enum2_t is (a1, b1);
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_no_messages(&messages);
    }

    #[test]
    fn forbid_homograph_to_enum_literals() {
        let code = Code::new(
            "
type enum_t is (a1, b1);
constant a1 : natural := 0;
function b1 return natural;
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_in_interface_file_declarations() {
        let code = Code::new(
            "
procedure proc(file a1, a, a1 : text);
",
        );

        let mut messages = Vec::new();
        check_declarative_part_unique_ident(&code.declarative_part(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_in_interface_type_declarations() {
        let code = Code::new(
            "
entity ent is
  generic (
    type a1;
    type a1
  );
end entity;
",
        );

        let mut messages = Vec::new();
        check_entity_declaration(&code.entity(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_in_interface_package_declarations() {
        let code = Code::new(
            "
entity ent is
  generic (
    package a1 is new pkg generic map (<>);
    package a1 is new pkg generic map (<>)
  );
end entity;
",
        );

        let mut messages = Vec::new();
        check_entity_declaration(&code.entity(), &mut messages);
        check_messages(messages, expected_messages(&code, &["a1"]));
    }

    #[test]
    fn forbid_homographs_in_entity_extended_declarative_regions() {
        let mut builder = LibraryBuilder::new();
        let ent = builder.code(
            "libname",
            "
entity ent is
  generic (
    constant g1 : natural;
    constant g2 : natural;
    constant g3 : natural;
    constant g4 : natural
  );
  port (
    signal g1 : natural;
    signal p1 : natural;
    signal p2 : natural;
    signal p3 : natural
  );
  constant g2 : natural := 0;
  constant p1 : natural := 0;
  constant e1 : natural := 0;
  constant e2 : natural := 0;
end entity;",
        );

        let arch1 = builder.code(
            "libname",
            "
architecture rtl of ent is
  constant g3 : natural := 0;
  constant p2 : natural := 0;
  constant e1 : natural := 0;
  constant a1 : natural := 0;
begin
end architecture;",
        );

        let arch2 = builder.code(
            "libname",
            "
architecture rtl2 of ent is
  constant a1 : natural := 0;
  constant e2 : natural := 0;
begin
end architecture;
",
        );

        let messages = builder.analyze();
        let mut expected = expected_messages(&ent, &["g1", "g2", "p1"]);
        expected.append(&mut expected_messages_multi(
            &ent,
            &arch1,
            &["g3", "p2", "e1"],
        ));
        expected.append(&mut expected_messages_multi(&ent, &arch2, &["e2"]));
        check_messages(messages, expected);
    }

    #[test]
    fn forbid_homographs_in_package_extended_declarative_regions() {
        let mut builder = LibraryBuilder::new();
        let pkg = builder.code(
            "libname",
            "
package pkg is
  generic (
    constant g1 : natural;
    constant g2 : natural
  );
  constant g1 : natural := 0;
end package;",
        );

        let body = builder.code(
            "libname",
            "
package body pkg is
  constant g1 : natural := 0;
  constant g2 : natural := 0;
  constant p1 : natural := 0;
end package body;",
        );

        let messages = builder.analyze();
        let mut expected = expected_messages(&pkg, &["g1"]);
        expected.append(&mut expected_messages_multi(&pkg, &body, &["g1", "g2"]));
        check_messages(messages, expected);
    }

    #[test]
    fn check_library_clause_library_exists() {
        let mut builder = LibraryBuilder::new();
        let code = builder.code(
            "libname",
            "
library missing_lib;

entity ent is
end entity;
            ",
        );

        let messages = builder.analyze();

        check_messages(
            messages,
            vec![Message::error(
                code.s1("missing_lib"),
                "No such library 'missing_lib'",
            )],
        )
    }

    #[test]
    fn library_std_is_pre_defined() {
        let mut builder = LibraryBuilder::new();
        builder.code(
            "libname",
            "
library std;

entity ent is
end entity;
            ",
        );

        let messages = builder.analyze();
        check_no_messages(&messages);
    }

    #[test]
    fn work_library_not_necessary_hint() {
        let mut builder = LibraryBuilder::new();
        let code = builder.code(
            "libname",
            "
library work;

entity ent is
end entity;
            ",
        );

        let messages = builder.analyze();

        check_messages(
            messages,
            vec![Message::hint(
                code.s1("work"),
                "Library clause not necessary for current working library",
            )],
        )
    }

    struct LibraryBuilder {
        code_builder: CodeBuilder,
        libraries: FnvHashMap<Symbol, Vec<Code>>,
    }

    impl LibraryBuilder {
        fn new() -> LibraryBuilder {
            LibraryBuilder {
                code_builder: CodeBuilder::new(),
                libraries: FnvHashMap::default(),
            }
        }
        fn code(&mut self, library_name: &str, code: &str) -> Code {
            let code = self.code_builder.code(code);
            let library_name = self.code_builder.symbol(library_name);
            match self.libraries.entry(library_name) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().push(code.clone());
                }
                Entry::Vacant(entry) => {
                    entry.insert(vec![code.clone()]);
                }
            }
            code
        }

        fn analyze(&self) -> Vec<Message> {
            let mut root = DesignRoot::new();
            let mut messages = Vec::new();

            for (library_name, codes) in self.libraries.iter() {
                let design_files = codes.iter().map(|code| code.design_file()).collect();
                let library = Library::new(library_name.clone(), design_files, &mut messages);
                root.add_library(library);
            }

            Analyzer::new(self.code_builder.symtab.clone()).analyze(&root, &mut messages);

            messages
        }
    }
}
