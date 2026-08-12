use rustc_lint::LateContext;
use rustc_middle::ty::{AssocKind, AssocTypeData, TyKind};
use rustc_span::def_id::DefId;

/// Determine whether a local trait is a static interpreter over an explicit
/// `World`. Additional associated types represent implementation-specific
/// resources; associated constants and receiver methods belong to ordinary
/// traits instead.
pub(crate) fn rvs_is_local_world_port_trait_S(cx: &LateContext<'_>, trait_def_id: DefId) -> bool {
    if !trait_def_id.is_local() {
        return false;
    }

    let associated_items = cx.tcx.associated_items(trait_def_id);
    let mut world_def_id = None;
    let mut operation_def_ids = Vec::new();
    for item in associated_items.in_definition_order() {
        match item.kind {
            AssocKind::Type {
                data: AssocTypeData::Normal(name),
            } if name.as_str() == "World" => {
                if world_def_id.replace(item.def_id).is_some()
                    || !cx.tcx.generics_of(item.def_id).own_params.is_empty()
                {
                    return false;
                }
            }
            AssocKind::Type { .. } => {}
            AssocKind::Fn { has_self, .. } => {
                if has_self {
                    return false;
                }
                operation_def_ids.push(item.def_id);
            }
            AssocKind::Const { .. } => return false,
        }
    }

    let Some(world_def_id) = world_def_id else {
        return false;
    };
    !operation_def_ids.is_empty()
        && operation_def_ids.into_iter().all(|operation_def_id| {
            cx.tcx
                .fn_sig(operation_def_id)
                .skip_binder()
                .inputs()
                .skip_binder()
                .iter()
                .any(|input| {
                    let TyKind::Ref(_, referent, _) = input.kind() else {
                        return false;
                    };
                    let TyKind::Alias(alias) = referent.kind() else {
                        return false;
                    };
                    alias.kind.def_id() == world_def_id
                })
        })
}
