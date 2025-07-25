use crate::place::{Place, imported_symbol, place_from_bindings, place_from_declarations};
use crate::semantic_index::definition::DefinitionKind;
use crate::semantic_index::place::ScopeId;
use crate::semantic_index::{
    attribute_scopes, global_scope, imported_modules, place_table, semantic_index, use_def_map,
};
use crate::types::{ClassBase, ClassLiteral, KnownClass, KnownInstanceType, Type};
use crate::{Db, NameKind};
use ruff_db::files::File;
use ruff_python_ast as ast;
use ruff_python_ast::name::Name;
use rustc_hash::FxHashSet;

pub(crate) fn all_declarations_and_bindings<'db>(
    db: &'db dyn Db,
    scope_id: ScopeId<'db>,
) -> impl Iterator<Item = Name> + 'db {
    let use_def_map = use_def_map(db, scope_id);
    let table = place_table(db, scope_id);

    use_def_map
        .all_end_of_scope_declarations()
        .filter_map(move |(symbol_id, declarations)| {
            place_from_declarations(db, declarations)
                .ok()
                .and_then(|result| {
                    result
                        .place
                        .ignore_possibly_unbound()
                        .and_then(|_| table.place_expr(symbol_id).as_name().cloned())
                })
        })
        .chain(
            use_def_map
                .all_end_of_scope_bindings()
                .filter_map(move |(symbol_id, bindings)| {
                    place_from_bindings(db, bindings)
                        .ignore_possibly_unbound()
                        .and_then(|_| table.place_expr(symbol_id).as_name().cloned())
                }),
        )
}

struct AllMembers {
    members: FxHashSet<Name>,
}

impl AllMembers {
    fn of<'db>(db: &'db dyn Db, ty: Type<'db>) -> Self {
        let mut all_members = Self {
            members: FxHashSet::default(),
        };
        all_members.extend_with_type(db, ty);
        all_members
    }

    fn extend_with_type<'db>(&mut self, db: &'db dyn Db, ty: Type<'db>) {
        match ty {
            Type::Union(union) => self.members.extend(
                union
                    .elements(db)
                    .iter()
                    .map(|ty| AllMembers::of(db, *ty).members)
                    .reduce(|acc, members| acc.intersection(&members).cloned().collect())
                    .unwrap_or_default(),
            ),

            Type::Intersection(intersection) => self.members.extend(
                intersection
                    .positive(db)
                    .iter()
                    .map(|ty| AllMembers::of(db, *ty).members)
                    .reduce(|acc, members| acc.union(&members).cloned().collect())
                    .unwrap_or_default(),
            ),

            Type::NominalInstance(instance) => {
                let (class_literal, _specialization) = instance.class.class_literal(db);
                self.extend_with_class_members(db, class_literal);
                self.extend_with_instance_members(db, class_literal);
            }

            Type::ClassLiteral(class_literal) => {
                self.extend_with_class_members(db, class_literal);

                if let Type::ClassLiteral(meta_class_literal) = ty.to_meta_type(db) {
                    self.extend_with_class_members(db, meta_class_literal);
                }
            }

            Type::GenericAlias(generic_alias) => {
                let class_literal = generic_alias.origin(db);
                self.extend_with_class_members(db, class_literal);
            }

            Type::SubclassOf(subclass_of_type) => {
                if let Some(class_literal) = subclass_of_type.subclass_of().into_class() {
                    self.extend_with_class_members(db, class_literal.class_literal(db).0);
                }
            }

            Type::Dynamic(_) | Type::Never | Type::AlwaysTruthy | Type::AlwaysFalsy => {}

            Type::IntLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::StringLiteral(_)
            | Type::BytesLiteral(_)
            | Type::LiteralString
            | Type::Tuple(_)
            | Type::PropertyInstance(_)
            | Type::FunctionLiteral(_)
            | Type::BoundMethod(_)
            | Type::MethodWrapper(_)
            | Type::WrapperDescriptor(_)
            | Type::DataclassDecorator(_)
            | Type::DataclassTransformer(_)
            | Type::Callable(_)
            | Type::ProtocolInstance(_)
            | Type::SpecialForm(_)
            | Type::KnownInstance(_)
            | Type::TypeVar(_)
            | Type::BoundSuper(_)
            | Type::TypeIs(_) => match ty.to_meta_type(db) {
                Type::ClassLiteral(class_literal) => {
                    self.extend_with_class_members(db, class_literal);
                }
                Type::GenericAlias(generic_alias) => {
                    let class_literal = generic_alias.origin(db);
                    self.extend_with_class_members(db, class_literal);
                }
                _ => {}
            },

            Type::ModuleLiteral(literal) => {
                self.extend_with_type(db, KnownClass::ModuleType.to_instance(db));
                let module = literal.module(db);

                let Some(file) = module.file() else {
                    return;
                };

                let module_scope = global_scope(db, file);
                let use_def_map = use_def_map(db, module_scope);
                let place_table = place_table(db, module_scope);

                for (symbol_id, _) in use_def_map.all_end_of_scope_declarations() {
                    let Some(symbol_name) = place_table.place_expr(symbol_id).as_name() else {
                        continue;
                    };
                    let Place::Type(ty, _) = imported_symbol(db, file, symbol_name, None).place
                    else {
                        continue;
                    };

                    // Filter private symbols from stubs if they appear to be internal types
                    let is_stub_file = file.path(db).extension() == Some("pyi");
                    let is_private_symbol = match NameKind::classify(symbol_name) {
                        NameKind::Dunder | NameKind::Normal => false,
                        NameKind::Sunder => true,
                    };
                    if is_private_symbol && is_stub_file {
                        match ty {
                            Type::NominalInstance(instance)
                                if matches!(
                                    instance.class.known(db),
                                    Some(
                                        KnownClass::TypeVar
                                            | KnownClass::TypeVarTuple
                                            | KnownClass::ParamSpec
                                    )
                                ) =>
                            {
                                continue;
                            }
                            Type::ClassLiteral(class) if class.is_protocol(db) => continue,
                            Type::KnownInstance(
                                KnownInstanceType::TypeVar(_) | KnownInstanceType::TypeAliasType(_),
                            ) => continue,
                            _ => {}
                        }
                    }

                    self.members
                        .insert(place_table.place_expr(symbol_id).expect_name().clone());
                }

                let module_name = module.name();
                self.members.extend(
                    imported_modules(db, literal.importing_file(db))
                        .iter()
                        .filter_map(|submodule_name| submodule_name.relative_to(module_name))
                        .filter_map(|relative_submodule_name| {
                            Some(Name::from(relative_submodule_name.components().next()?))
                        }),
                );
            }
        }
    }

    fn extend_with_declarations_and_bindings(&mut self, db: &dyn Db, scope_id: ScopeId) {
        self.members
            .extend(all_declarations_and_bindings(db, scope_id));
    }

    fn extend_with_class_members<'db>(
        &mut self,
        db: &'db dyn Db,
        class_literal: ClassLiteral<'db>,
    ) {
        for parent in class_literal
            .iter_mro(db, None)
            .filter_map(ClassBase::into_class)
            .map(|class| class.class_literal(db).0)
        {
            let parent_scope = parent.body_scope(db);
            self.extend_with_declarations_and_bindings(db, parent_scope);
        }
    }

    fn extend_with_instance_members<'db>(
        &mut self,
        db: &'db dyn Db,
        class_literal: ClassLiteral<'db>,
    ) {
        for parent in class_literal
            .iter_mro(db, None)
            .filter_map(ClassBase::into_class)
            .map(|class| class.class_literal(db).0)
        {
            let class_body_scope = parent.body_scope(db);
            let file = class_body_scope.file(db);
            let index = semantic_index(db, file);
            for function_scope_id in attribute_scopes(db, class_body_scope) {
                let place_table = index.place_table(function_scope_id);
                self.members
                    .extend(place_table.instance_attributes().cloned());
            }
        }
    }
}

/// List all members of a given type: anything that would be valid when accessed
/// as an attribute on an object of the given type.
pub fn all_members<'db>(db: &'db dyn Db, ty: Type<'db>) -> FxHashSet<Name> {
    AllMembers::of(db, ty).members
}

/// Get the primary definition kind for a name expression within a specific file.
/// Returns the first definition kind that is reachable for this name in its scope.
/// This is useful for IDE features like semantic tokens.
pub fn definition_kind_for_name<'db>(
    db: &'db dyn Db,
    file: File,
    name: &ast::ExprName,
) -> Option<DefinitionKind<'db>> {
    let index = semantic_index(db, file);
    let name_str = name.id.as_str();

    // Get the scope for this name expression
    let file_scope = index.try_expression_scope_id(&ast::Expr::Name(name.clone()))?;

    // Get the place table for this scope
    let place_table = index.place_table(file_scope);

    // Look up the place by name
    let place_id = place_table.place_id_by_name(name_str)?;

    // Get the use-def map and look up definitions for this place
    let use_def_map = index.use_def_map(file_scope);
    let declarations = use_def_map.all_reachable_declarations(place_id);

    // Find the first valid definition and return its kind
    for declaration in declarations {
        if let Some(def) = declaration.declaration.definition() {
            return Some(def.kind(db).clone());
        }
    }

    None
}
