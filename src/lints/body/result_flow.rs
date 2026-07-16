use std::collections::{HashMap, HashSet, VecDeque};

use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_lint::LateContext;
use rustc_middle::mir::{
    AggregateKind, BasicBlock, BorrowKind, InlineAsmOperand, Local, NonDivergingIntrinsic, Operand,
    Place, ProjectionElem, RETURN_PLACE, Rvalue, START_BLOCK, StatementKind, TerminatorKind,
};
use rustc_middle::ty::{Ty, TyKind};
use rustc_span::sym;

use super::super::utils::rvs_def_path;
use crate::symbols::DefPath;

const RVS_RESULT_UNINITIALIZED: u8 = 1;
const RVS_RESULT_OK: u8 = 2;
const RVS_RESULT_ERR: u8 = 4;
const RVS_RESULT_PENDING: u8 = 8;
const RVS_RESULT_UNKNOWN: u8 = RVS_RESULT_OK | RVS_RESULT_ERR;
const RVS_RESULT_ANY: u8 = RVS_RESULT_UNKNOWN | RVS_RESULT_PENDING;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResultProjectionKey {
    display: String,
    sibling_fields_overlap: bool,
    imprecise_index: bool,
}

type ResultPlaceKey = (Local, Vec<ResultProjectionKey>);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ResultReference {
    // Direct reaches the observed Result itself; nested reaches Results carried in its payload.
    direct: bool,
    direct_mutable: bool,
    nested: bool,
    nested_mutable: bool,
}

impl ResultReference {
    fn rvs_direct_mutable() -> Self {
        Self {
            direct: true,
            direct_mutable: true,
            nested: false,
            nested_mutable: false,
        }
    }

    fn rvs_nested_mutable() -> Self {
        Self {
            direct: false,
            direct_mutable: false,
            nested: true,
            nested_mutable: true,
        }
    }

    fn rvs_has_reference(self) -> bool {
        self.direct || self.nested
    }

    fn rvs_can_change_variant(self) -> bool {
        self.direct_mutable || self.nested_mutable
    }

    fn rvs_preserve_direct_variant(self) -> Self {
        Self {
            direct_mutable: false,
            ..self
        }
    }

    fn rvs_nest(self) -> Self {
        Self {
            direct: false,
            direct_mutable: false,
            nested: self.direct || self.nested,
            nested_mutable: self.direct_mutable || self.nested_mutable,
        }
    }

    fn rvs_join_M(&mut self, other: Self) {
        self.direct |= other.direct;
        self.direct_mutable |= other.direct_mutable;
        self.nested |= other.nested;
        self.nested_mutable |= other.nested_mutable;
    }
}

type ResultReferenceMap = HashMap<ResultPlaceKey, ResultReference>;
type SummaryAliases = HashMap<Local, ResultReference>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResultDiscriminant {
    source: ResultPlaceKey,
    is_poll: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LocalParamChannelEffect {
    may_mutate: bool,
    may_escape: bool,
    may_drop: bool,
    may_return_alias: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LocalParamEffect {
    direct: LocalParamChannelEffect,
    nested: LocalParamChannelEffect,
}

impl LocalParamEffect {
    fn rvs_preserves_argument(self, argument_needs_drop: bool) -> bool {
        [self.direct, self.nested].into_iter().all(|effect| {
            !effect.may_mutate && !effect.may_escape && !(effect.may_drop && argument_needs_drop)
        })
    }

    fn rvs_may_mutate(self, reference: ResultReference) -> bool {
        (reference.direct_mutable && self.direct.may_mutate)
            || (reference.nested_mutable && self.nested.may_mutate)
    }

    fn rvs_may_escape(self, reference: ResultReference) -> bool {
        (reference.direct_mutable && self.direct.may_escape)
            || (reference.nested_mutable && self.nested.may_escape)
    }

    fn rvs_may_drop(self, reference: ResultReference) -> bool {
        (reference.direct_mutable && self.direct.may_drop)
            || (reference.nested_mutable && self.nested.may_drop)
    }

    fn rvs_may_return_alias(self, reference: ResultReference) -> bool {
        (reference.direct && self.direct.may_return_alias)
            || (reference.nested && self.nested.may_return_alias)
    }
}

fn rvs_unknown_local_param_effect() -> LocalParamEffect {
    let channel = LocalParamChannelEffect {
        may_mutate: true,
        may_escape: true,
        may_drop: true,
        may_return_alias: true,
    };
    LocalParamEffect {
        direct: channel,
        nested: channel,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResultFlowState {
    values: HashMap<Local, u8>,
    projected_values: HashMap<ResultPlaceKey, u8>,
    references: HashMap<Local, ResultReferenceMap>,
    discriminants: HashMap<Local, ResultDiscriminant>,
    predicates: HashMap<Local, (ResultPlaceKey, bool)>,
    escaped_results: HashSet<ResultPlaceKey>,
    // Saved coroutine fields remain valid on resume edges that bypass their initialization block.
    coroutine_saved_values: HashSet<ResultPlaceKey>,
}

impl ResultFlowState {
    fn rvs_join_M(&mut self, other: &Self) -> bool {
        let mut changed = false;
        for (local, value) in &other.values {
            let entry = self
                .values
                .entry(*local)
                .or_insert(RVS_RESULT_UNINITIALIZED);
            let joined = *entry | *value;
            changed |= joined != *entry;
            *entry = joined;
        }
        for (local, references) in &other.references {
            let entry = self.references.entry(*local).or_default();
            for (target, reference) in references {
                let previous = entry.get(target).copied().unwrap_or_default();
                let mut joined = previous;
                joined.rvs_join_M(*reference);
                changed |= previous != joined || !entry.contains_key(target);
                entry.insert(target.clone(), joined);
            }
        }
        for (place, value) in &mut self.projected_values {
            if !other.projected_values.contains_key(place)
                && !self.coroutine_saved_values.contains(place)
            {
                let joined = *value | RVS_RESULT_ANY;
                changed |= joined != *value;
                *value = joined;
            }
        }
        for (place, value) in &other.projected_values {
            if let Some(entry) = self.projected_values.get_mut(place) {
                let joined = *entry | *value;
                changed |= joined != *entry;
                *entry = joined;
            } else {
                let value = if other.coroutine_saved_values.contains(place) {
                    *value
                } else {
                    *value | RVS_RESULT_ANY
                };
                self.projected_values.insert(place.clone(), value);
                changed = true;
            }
        }
        let saved_count = self.coroutine_saved_values.len();
        self.coroutine_saved_values
            .extend(other.coroutine_saved_values.iter().cloned());
        changed |= self.coroutine_saved_values.len() != saved_count;
        let existing_discriminants = self.discriminants.clone();
        self.discriminants.retain(|local, source| {
            other
                .discriminants
                .get(local)
                .is_some_and(|other| other == source)
        });
        changed |= self.discriminants != existing_discriminants;
        let existing_predicates = self.predicates.clone();
        self.predicates.retain(|local, predicate| {
            other
                .predicates
                .get(local)
                .is_some_and(|other| other == predicate)
        });
        changed |= self.predicates != existing_predicates;
        let escaped_count = self.escaped_results.len();
        self.escaped_results
            .extend(other.escaped_results.iter().cloned());
        changed |= self.escaped_results.len() != escaped_count;
        changed
    }
}

pub(crate) fn rvs_mir_has_potential_error_return(
    cx: &LateContext<'_>,
    owner: LocalDefId,
) -> Option<bool> {
    rvs_mir_has_potential_error_return_inner(cx, owner, &mut HashSet::new())
}

fn rvs_mir_has_potential_error_return_inner(
    cx: &LateContext<'_>,
    owner: LocalDefId,
    visiting: &mut HashSet<LocalDefId>,
) -> Option<bool> {
    if !visiting.insert(owner) {
        return Some(false);
    }
    if !cx.tcx.is_mir_available(owner.to_def_id()) {
        visiting.remove(&owner);
        return None;
    }
    let body = cx.tcx.optimized_mir(owner);
    let return_type = rvs_local_type(body, RETURN_PLACE);
    let result = if rvs_result_error_type(cx, return_type).is_some() {
        Some(rvs_analyze_result_body(cx, body, visiting))
    } else {
        let return_type = cx
            .tcx
            .try_normalize_erasing_regions(cx.typing_env(), return_type)
            .unwrap_or(return_type);
        let TyKind::Coroutine(coroutine, _) = return_type.kind() else {
            visiting.remove(&owner);
            return None;
        };
        let Some(coroutine) = coroutine.as_local() else {
            visiting.remove(&owner);
            return None;
        };
        rvs_mir_has_potential_error_return_inner(cx, coroutine, visiting)
    };
    visiting.remove(&owner);
    result
}

fn rvs_analyze_result_body<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    visiting: &mut HashSet<LocalDefId>,
) -> bool {
    let return_type = rvs_local_type(body, RETURN_PLACE);
    debug_assert!(
        rvs_result_error_type(cx, return_type).is_some(),
        "result flow body returns Result or Poll<Result>"
    );

    let mut initial_values = HashMap::new();
    for (local, declaration) in body.local_decls.iter_enumerated() {
        if rvs_result_error_type(cx, declaration.ty).is_some() {
            initial_values.insert(local, RVS_RESULT_UNINITIALIZED);
        }
    }
    for argument in body.args_iter() {
        if let Some(value) = initial_values.get_mut(&argument) {
            *value = rvs_unknown_result_value(cx, rvs_local_type(body, argument));
        }
    }
    let initial = ResultFlowState {
        values: initial_values,
        projected_values: HashMap::new(),
        references: HashMap::new(),
        discriminants: HashMap::new(),
        predicates: HashMap::new(),
        escaped_results: HashSet::new(),
        coroutine_saved_values: HashSet::new(),
    };
    let local_effects = rvs_local_effect_summaries(cx, body);
    let mut entries = HashMap::from([(START_BLOCK, initial)]);
    let mut pending = VecDeque::from([START_BLOCK]);
    let mut terminal_errors = HashMap::new();

    while let Some(block) = pending.pop_front() {
        let Some(mut state) = entries.get(&block).cloned() else {
            continue;
        };
        let block_data = body
            .basic_blocks
            .get(block)
            .expect("never: pending MIR block belongs to this body");
        for statement in &block_data.statements {
            rvs_apply_statement_M(cx, body, &mut state, &statement.kind);
        }
        let terminator = block_data.terminator();
        rvs_apply_terminator_effects_M(
            cx,
            body,
            &mut state,
            &terminator.kind,
            visiting,
            &local_effects,
        );
        match &terminator.kind {
            TerminatorKind::Return => {
                let value = state
                    .values
                    .get(&RETURN_PLACE)
                    .copied()
                    .unwrap_or(RVS_RESULT_UNKNOWN);
                terminal_errors.insert(
                    block,
                    value & (RVS_RESULT_ERR | RVS_RESULT_UNINITIALIZED) != 0,
                );
            }
            TerminatorKind::TailCall { func, .. } => {
                let callee_summary =
                    rvs_operand_def_id(func)
                        .and_then(DefId::as_local)
                        .and_then(|callee| {
                            rvs_mir_has_potential_error_return_inner(cx, callee, visiting)
                        });
                terminal_errors.insert(
                    block,
                    callee_summary.unwrap_or_else(|| {
                        rvs_unknown_result_value(cx, return_type) & RVS_RESULT_ERR != 0
                    }),
                );
            }
            TerminatorKind::SwitchInt { discr, targets } => {
                let discriminant_local = rvs_operand_local(&discr);
                let source =
                    discriminant_local.and_then(|local| state.discriminants.get(&local).cloned());
                let predicate =
                    discriminant_local.and_then(|local| state.predicates.get(&local).cloned());
                let explicit_targets = targets.iter().collect::<Vec<_>>();
                for (value, target) in &explicit_targets {
                    let mut edge = state.clone();
                    if rvs_refine_switch_edge_M(
                        cx,
                        body,
                        &mut edge,
                        source.clone(),
                        predicate.clone(),
                        *value,
                    ) {
                        rvs_merge_block_entry_M(&mut entries, &mut pending, *target, edge);
                    }
                }
                let otherwise_value = match explicit_targets.as_slice() {
                    [(0, _)] => Some(1),
                    [(1, _)] => Some(0),
                    _ => None,
                };
                if let Some(otherwise_value) = otherwise_value {
                    let mut edge = state;
                    if rvs_refine_switch_edge_M(
                        cx,
                        body,
                        &mut edge,
                        source,
                        predicate,
                        otherwise_value,
                    ) {
                        rvs_merge_block_entry_M(
                            &mut entries,
                            &mut pending,
                            targets.otherwise(),
                            edge,
                        );
                    }
                } else {
                    rvs_merge_block_entry_M(&mut entries, &mut pending, targets.otherwise(), state);
                }
            }
            _ => {
                for successor in terminator.successors() {
                    rvs_merge_block_entry_M(&mut entries, &mut pending, successor, state.clone());
                }
            }
        }
    }

    terminal_errors.into_values().any(|has_error| has_error)
}

fn rvs_merge_block_entry_M(
    entries: &mut HashMap<BasicBlock, ResultFlowState>,
    pending: &mut VecDeque<BasicBlock>,
    block: BasicBlock,
    incoming: ResultFlowState,
) {
    let changed = if let Some(existing) = entries.get_mut(&block) {
        existing.rvs_join_M(&incoming)
    } else {
        entries.insert(block, incoming);
        true
    };
    if changed {
        pending.push_back(block);
    }
}

fn rvs_apply_statement_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &mut ResultFlowState,
    statement: &StatementKind<'tcx>,
) {
    match statement {
        StatementKind::Assign(assignment) => {
            let (destination, value) = &**assignment;
            let destination_key = rvs_projected_place_key(cx, body, *destination);
            if destination.projection.is_empty() {
                state
                    .projected_values
                    .retain(|(local, _), _| *local != destination.local);
                state
                    .coroutine_saved_values
                    .retain(|(local, _)| *local != destination.local);
                rvs_invalidate_relations_for_source_M(state, &destination_key);
            }
            let destination_is_result =
                rvs_result_error_type(cx, destination.ty(&body.local_decls, cx.tcx).ty).is_some();
            let result_value = rvs_rvalue_result_value(cx, body, state, destination, value);
            if !destination.projection.is_empty() {
                let destination_projection = &destination_key.1;
                state.projected_values.retain(|(local, projection), _| {
                    *local != destination.local
                        || !rvs_projection_paths_overlap(projection, destination_projection)
                });
                state.coroutine_saved_values.retain(|(local, projection)| {
                    *local != destination.local
                        || !rvs_projection_paths_overlap(projection, destination_projection)
                });
                rvs_invalidate_relations_for_source_M(state, &destination_key);
            }
            rvs_assign_result_value_M(
                cx,
                body,
                state,
                destination,
                result_value,
                destination_is_result,
            );
            if destination_is_result
                && !destination.projection.is_empty()
                && rvs_type_coroutine_def_id(rvs_local_type(body, destination.local)).is_some()
            {
                state.coroutine_saved_values.insert(destination_key.clone());
            }
            let copied_predicate = match value {
                Rvalue::Use(operand) => rvs_operand_local(operand)
                    .and_then(|local| state.predicates.get(&local).cloned()),
                _ => None,
            };
            rvs_assign_references_M(cx, body, state, destination, value);
            if destination.projection.is_empty() {
                if let Rvalue::Discriminant(source) = value {
                    let source_type = source.ty(&body.local_decls, cx.tcx).ty;
                    let source_key = rvs_projected_place_key(cx, body, *source);
                    let tracks_result = rvs_result_error_type(cx, source_type).is_some()
                        || (source.projection.is_empty()
                            && state.values.contains_key(&source.local))
                        || state.projected_values.contains_key(&source_key);
                    if tracks_result {
                        state.discriminants.insert(
                            destination.local,
                            ResultDiscriminant {
                                source: source_key,
                                is_poll: rvs_poll_result_type(cx, source_type).is_some(),
                            },
                        );
                    } else {
                        state.discriminants.remove(&destination.local);
                    }
                } else {
                    state.discriminants.remove(&destination.local);
                }
                if let Some(predicate) = copied_predicate {
                    state.predicates.insert(destination.local, predicate);
                } else {
                    state.predicates.remove(&destination.local);
                }
            }
        }
        StatementKind::SetDiscriminant {
            place,
            variant_index,
        } => {
            let place_type = place.ty(&body.local_decls, cx.tcx).ty;
            let value = rvs_result_variant_value(cx, body, state, place_type, *variant_index, None)
                .unwrap_or(RVS_RESULT_UNKNOWN);
            rvs_assign_result_value_M(
                cx,
                body,
                state,
                place,
                value,
                rvs_result_error_type(cx, place_type).is_some(),
            );
        }
        StatementKind::Intrinsic(intrinsic) => {
            if let NonDivergingIntrinsic::CopyNonOverlapping(copy) = &**intrinsic {
                for (target, reference) in rvs_operand_references(cx, body, state, &copy.dst) {
                    if reference.rvs_can_change_variant() {
                        rvs_set_result_target_unknown_M(cx, body, state, &target);
                    }
                }
            }
        }
        StatementKind::StorageDead(local) => {
            if state.values.contains_key(local) {
                state.values.insert(*local, RVS_RESULT_UNINITIALIZED);
            }
            state.references.remove(local);
            state
                .projected_values
                .retain(|(projected_local, _), _| projected_local != local);
            state
                .coroutine_saved_values
                .retain(|(projected_local, _)| projected_local != local);
            state.discriminants.remove(local);
            state.predicates.remove(local);
        }
        _ => {}
    }
}

fn rvs_apply_terminator_effects_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &mut ResultFlowState,
    terminator: &TerminatorKind<'tcx>,
    visiting: &mut HashSet<LocalDefId>,
    local_effects: &HashMap<DefId, Vec<LocalParamEffect>>,
) {
    if matches!(terminator, TerminatorKind::InlineAsm { .. }) {
        let mut aliased = state
            .references
            .values()
            .flat_map(|references| references.iter())
            .filter_map(|(target, reference)| {
                reference.rvs_can_change_variant().then_some(target.clone())
            })
            .collect::<HashSet<_>>();
        aliased.extend(state.escaped_results.iter().cloned());
        for target in aliased {
            state.escaped_results.insert(target.clone());
            rvs_set_result_target_unknown_M(cx, body, state, &target);
        }
        return;
    }
    if let TerminatorKind::Drop { place, .. } = terminator {
        let references = rvs_place_references(cx, body, state, place);
        for (target, reference) in references {
            if reference.rvs_can_change_variant() {
                rvs_set_result_target_unknown_M(cx, body, state, &target);
            }
        }
        return;
    }
    let TerminatorKind::Call {
        func,
        args,
        destination,
        ..
    } = terminator
    else {
        return;
    };
    let called_def_id = rvs_operand_def_id(func);
    for target in state.escaped_results.clone() {
        rvs_set_result_target_unknown_M(cx, body, state, &target);
    }
    let called_path = called_def_id.map(|def_id| {
        DefPath::rvs_new(rvs_def_path(cx, def_id))
            .rvs_user_path()
            .into_owned()
    });
    let preserves_result_variant =
        called_def_id.is_some_and(|def_id| rvs_call_preserves_result_variant(cx, def_id));
    let argument_reference_maps = args
        .iter()
        .map(|argument| rvs_operand_references(cx, body, state, &argument.node))
        .collect::<Vec<_>>();
    let argument_references = rvs_merge_reference_maps(argument_reference_maps.iter().cloned());
    let local_param_effects = called_def_id.and_then(|def_id| local_effects.get(&def_id));
    let local_preserves_argument_contents = local_param_effects.is_some_and(|effects| {
        effects.iter().enumerate().all(|(index, effect)| {
            let argument_needs_drop = args.get(index).is_some_and(|argument| {
                argument
                    .node
                    .ty(&body.local_decls, cx.tcx)
                    .needs_drop(cx.tcx, cx.typing_env())
            });
            effect.rvs_preserves_argument(argument_needs_drop)
        })
    });
    let preserves_argument_contents = local_preserves_argument_contents
        || called_def_id.is_some_and(|def_id| rvs_call_preserves_argument_contents(cx, def_id));
    let preserved_result_value = preserves_result_variant
        .then(|| {
            argument_references
                .iter()
                .filter(|(_, reference)| reference.direct)
                .map(|(target, _)| rvs_result_place_value(state, target))
                .fold(0, |combined, value| combined | value)
        })
        .filter(|value| *value != 0);
    if let Some(effects) = local_param_effects {
        for (index, references) in argument_reference_maps.iter().enumerate() {
            let effect = effects
                .get(index)
                .copied()
                .unwrap_or_else(rvs_unknown_local_param_effect);
            let argument_needs_drop = args.get(index).is_some_and(|argument| {
                argument
                    .node
                    .ty(&body.local_decls, cx.tcx)
                    .needs_drop(cx.tcx, cx.typing_env())
            });
            for (target, reference) in references {
                let may_mutate = effect.rvs_may_mutate(*reference);
                let may_escape = effect.rvs_may_escape(*reference);
                let may_drop = argument_needs_drop && effect.rvs_may_drop(*reference);
                if may_mutate || may_escape || may_drop {
                    if may_escape {
                        state.escaped_results.insert(target.clone());
                    }
                    rvs_set_result_target_unknown_M(cx, body, state, target);
                }
            }
        }
    } else if !preserves_result_variant && !preserves_argument_contents {
        for (target, reference) in &argument_references {
            if reference.rvs_can_change_variant() {
                state.escaped_results.insert(target.clone());
                rvs_set_result_target_unknown_M(cx, body, state, target);
            }
        }
    }
    if destination.projection.is_empty() {
        if state.values.contains_key(&destination.local) {
            state.values.insert(
                destination.local,
                rvs_unknown_result_value(cx, rvs_local_type(body, destination.local)),
            );
        }
        state.references.remove(&destination.local);
        let returned_references = if let Some(effects) = local_param_effects {
            rvs_merge_reference_maps(effects.iter().enumerate().filter_map(|(index, effect)| {
                argument_reference_maps.get(index).map(|references| {
                    references
                        .iter()
                        .filter(|(_, reference)| effect.rvs_may_return_alias(**reference))
                        .map(|(target, reference)| (target.clone(), *reference))
                        .collect()
                })
            }))
        } else {
            argument_references.clone()
        };
        if !returned_references.is_empty() {
            let references = if preserves_result_variant {
                rvs_preserve_direct_variants(returned_references)
            } else {
                returned_references
            };
            state.references.insert(destination.local, references);
        }
        state.discriminants.remove(&destination.local);
        state.predicates.remove(&destination.local);
    }
    if let Some(value) = preserved_result_value
        && state.values.contains_key(&destination.local)
    {
        state.values.insert(destination.local, value);
    }
    if called_path
        .as_deref()
        .is_some_and(rvs_call_is_result_from_output)
        && state.values.contains_key(&destination.local)
    {
        state.values.insert(destination.local, RVS_RESULT_OK);
    }
    if called_path
        .as_deref()
        .is_some_and(rvs_call_is_result_branch)
        && destination.projection.is_empty()
        && let Some(argument) = args.first()
    {
        let source_value = rvs_operand_result_value(cx, body, state, &argument.node);
        state.values.insert(destination.local, source_value);
    }
    if called_def_id.is_some_and(|def_id| rvs_call_is_identity(cx, def_id))
        && state.values.contains_key(&destination.local)
        && let Some(argument) = args.first()
    {
        let source_value = rvs_operand_result_value(cx, body, state, &argument.node);
        state.values.insert(destination.local, source_value);
    }
    if destination.projection.is_empty()
        && let Some((is_error_test, source)) =
            rvs_result_predicate_source(called_path.as_deref(), &argument_references)
    {
        state
            .predicates
            .insert(destination.local, (source, is_error_test));
    }
    let local_result_summary = called_def_id
        .and_then(DefId::as_local)
        .and_then(|callee| rvs_mir_has_potential_error_return_inner(cx, callee, visiting));
    if let Some(false) = local_result_summary
        && state.values.contains_key(&destination.local)
    {
        state.values.insert(destination.local, RVS_RESULT_OK);
    }
    if rvs_poll_result_type(cx, rvs_local_type(body, destination.local)).is_some()
        && local_result_summary != Some(true)
        && called_def_id.is_some_and(|def_id| rvs_call_is_future_poll(cx, def_id))
        && let Some(coroutine) = rvs_operand_direct_coroutine_def_id(func)
        && let Some(summary) = rvs_mir_has_potential_error_return_inner(cx, coroutine, visiting)
    {
        let completed = if summary {
            RVS_RESULT_UNKNOWN
        } else {
            RVS_RESULT_OK
        };
        state
            .values
            .insert(destination.local, completed | RVS_RESULT_PENDING);
    }
}

fn rvs_rvalue_result_value<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &ResultFlowState,
    destination: &Place<'tcx>,
    value: &Rvalue<'tcx>,
) -> u8 {
    let destination_type = destination.ty(&body.local_decls, cx.tcx).ty;
    if rvs_result_error_type(cx, destination_type).is_none() {
        return RVS_RESULT_UNKNOWN;
    }
    match value {
        Rvalue::Use(operand) => rvs_operand_result_value(cx, body, state, operand),
        Rvalue::Aggregate(kind, operands) => match &**kind {
            AggregateKind::Adt(_, variant_index, ..) => rvs_result_variant_value(
                cx,
                body,
                state,
                destination_type,
                *variant_index,
                operands.iter().next(),
            )
            .unwrap_or_else(|| rvs_unknown_result_value(cx, destination_type)),
            _ => rvs_unknown_result_value(cx, destination_type),
        },
        _ => rvs_unknown_result_value(cx, destination_type),
    }
}

fn rvs_operand_result_value<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &ResultFlowState,
    operand: &Operand<'tcx>,
) -> u8 {
    let Some(place) = rvs_operand_place(operand) else {
        return RVS_RESULT_UNKNOWN;
    };
    if place.projection.is_empty() {
        state
            .values
            .get(&place.local)
            .copied()
            .unwrap_or_else(|| rvs_unknown_result_value(cx, rvs_local_type(body, place.local)))
    } else {
        state
            .projected_values
            .get(&rvs_projected_place_key(cx, body, *place))
            .copied()
            .or_else(|| {
                (rvs_poll_result_type(cx, rvs_local_type(body, place.local)).is_some()
                    && rvs_is_exact_poll_ready_payload(place))
                .then(|| state.values.get(&place.local).copied())
                .flatten()
                .and_then(|value| {
                    let completed = value & (RVS_RESULT_OK | RVS_RESULT_ERR);
                    (completed != 0).then_some(completed)
                })
            })
            .unwrap_or_else(|| rvs_unknown_result_value(cx, place.ty(&body.local_decls, cx.tcx).ty))
    }
}

fn rvs_assign_result_value_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &mut ResultFlowState,
    place: &Place<'tcx>,
    value: u8,
    tracks_result: bool,
) {
    debug_assert_ne!(value, 0, "result state contains at least one possibility");
    debug_assert_eq!(
        value & !(RVS_RESULT_UNINITIALIZED | RVS_RESULT_ANY),
        0,
        "result state contains only known flags"
    );
    let place_key = rvs_projected_place_key(cx, body, *place);
    let escaped = state.escaped_results.iter().any(|escaped| {
        escaped.0 == place_key.0 && rvs_projection_paths_overlap(&escaped.1, &place_key.1)
    });
    if place.projection.is_empty() && state.values.contains_key(&place.local) {
        let value = if escaped { RVS_RESULT_UNKNOWN } else { value };
        state.values.insert(place.local, value);
        return;
    }
    if tracks_result && !place.projection.is_empty() {
        let value = if escaped { RVS_RESULT_UNKNOWN } else { value };
        state.projected_values.insert(place_key, value);
    }
    if place
        .projection
        .iter()
        .any(|projection| matches!(projection, rustc_middle::mir::ProjectionElem::Deref))
    {
        for (target, reference) in state
            .references
            .get(&place.local)
            .cloned()
            .unwrap_or_default()
        {
            if reference.rvs_can_change_variant() {
                rvs_invalidate_relations_for_source_M(state, &target);
                rvs_set_result_target_value_M(state, &target, value);
            }
        }
    }
}

fn rvs_assign_references_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &mut ResultFlowState,
    destination: &Place<'tcx>,
    value: &Rvalue<'tcx>,
) {
    let references = match value {
        Rvalue::Ref(_, BorrowKind::Mut { .. }, place) => {
            rvs_place_references(cx, body, state, place)
        }
        Rvalue::Ref(_, _, place) => {
            let references = rvs_place_references(cx, body, state, place);
            let place_type = place.ty(&body.local_decls, cx.tcx).ty;
            if rvs_result_error_type(cx, place_type).is_some() {
                rvs_preserve_direct_variants(references)
            } else {
                rvs_nest_reference_map(references)
            }
        }
        Rvalue::RawPtr(_, place) => rvs_place_references(cx, body, state, place),
        Rvalue::Cast(_, operand, _) => rvs_operand_references(cx, body, state, operand),
        Rvalue::Use(operand) | Rvalue::Repeat(operand, _) | Rvalue::UnaryOp(_, operand) => {
            rvs_operand_references(cx, body, state, operand)
        }
        Rvalue::BinaryOp(_, operands) => {
            let (left, right) = &**operands;
            rvs_merge_reference_maps([
                rvs_operand_references(cx, body, state, left),
                rvs_operand_references(cx, body, state, right),
            ])
        }
        Rvalue::Aggregate(_, operands) => rvs_nest_reference_map(rvs_merge_reference_maps(
            operands
                .iter()
                .map(|operand| rvs_operand_references(cx, body, state, operand)),
        )),
        _ => HashMap::new(),
    };
    if !destination.projection.is_empty() {
        if !references.is_empty() {
            let entry = state.references.entry(destination.local).or_default();
            for (target, reference) in rvs_nest_reference_map(references) {
                entry
                    .entry(target)
                    .and_modify(|existing| existing.rvs_join_M(reference))
                    .or_insert(reference);
            }
        }
        return;
    }
    state.references.remove(&destination.local);
    if !references.is_empty() {
        state.references.insert(destination.local, references);
    }
}

fn rvs_place_references<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &ResultFlowState,
    place: &Place<'tcx>,
) -> ResultReferenceMap {
    let mut references = state
        .references
        .get(&place.local)
        .cloned()
        .unwrap_or_default();
    let place_key = rvs_projected_place_key(cx, body, *place);
    let place_type = place.ty(&body.local_decls, cx.tcx).ty;
    if rvs_result_error_type(cx, place_type).is_some() {
        references
            .entry(place_key.clone())
            .and_modify(|existing| existing.rvs_join_M(ResultReference::rvs_direct_mutable()))
            .or_insert_with(ResultReference::rvs_direct_mutable);
    }
    for tracked in state.projected_values.keys() {
        if tracked.0 != place.local
            || !rvs_projection_paths_overlap(&tracked.1, &place_key.1)
            || tracked == &place_key
        {
            continue;
        }
        references
            .entry(tracked.clone())
            .and_modify(|existing| existing.rvs_join_M(ResultReference::rvs_nested_mutable()))
            .or_insert_with(ResultReference::rvs_nested_mutable);
    }
    references
}

fn rvs_operand_references<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &ResultFlowState,
    operand: &Operand<'tcx>,
) -> ResultReferenceMap {
    rvs_operand_place(operand).map_or_else(HashMap::new, |place| {
        rvs_place_references(cx, body, state, place)
    })
}

fn rvs_merge_reference_maps(
    maps: impl IntoIterator<Item = ResultReferenceMap>,
) -> ResultReferenceMap {
    let mut merged: ResultReferenceMap = HashMap::new();
    for references in maps {
        for (target, reference) in references {
            merged
                .entry(target)
                .and_modify(|existing| existing.rvs_join_M(reference))
                .or_insert(reference);
        }
    }
    merged
}

fn rvs_preserve_direct_variants(mut references: ResultReferenceMap) -> ResultReferenceMap {
    for reference in references.values_mut() {
        *reference = reference.rvs_preserve_direct_variant();
    }
    references
}

fn rvs_nest_reference_map(mut references: ResultReferenceMap) -> ResultReferenceMap {
    for reference in references.values_mut() {
        *reference = reference.rvs_nest();
    }
    references
}

fn rvs_operand_place<'a, 'tcx>(operand: &'a Operand<'tcx>) -> Option<&'a Place<'tcx>> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => Some(place),
        Operand::Constant(_) | Operand::RuntimeChecks(_) => None,
    }
}

fn rvs_operand_local(operand: &Operand<'_>) -> Option<Local> {
    let place = rvs_operand_place(operand)?;
    place.projection.is_empty().then_some(place.local)
}

fn rvs_operand_def_id(operand: &Operand<'_>) -> Option<DefId> {
    let Operand::Constant(constant) = operand else {
        return None;
    };
    let TyKind::FnDef(def_id, _) = constant.const_.ty().kind() else {
        return None;
    };
    Some(*def_id)
}

fn rvs_operand_direct_coroutine_def_id(operand: &Operand<'_>) -> Option<LocalDefId> {
    let Operand::Constant(constant) = operand else {
        return None;
    };
    let TyKind::FnDef(_, arguments) = constant.const_.ty().kind() else {
        return None;
    };
    arguments.types().find_map(|ty| match ty.kind() {
        TyKind::Coroutine(def_id, _) => def_id.as_local(),
        _ => None,
    })
}

fn rvs_type_coroutine_def_id(ty: Ty<'_>) -> Option<LocalDefId> {
    match ty.kind() {
        TyKind::Coroutine(def_id, _) => def_id.as_local(),
        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => rvs_type_coroutine_def_id(*inner),
        TyKind::Adt(_, arguments) => arguments.types().find_map(rvs_type_coroutine_def_id),
        _ => None,
    }
}

fn rvs_projected_place_key<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    place: Place<'tcx>,
) -> ResultPlaceKey {
    (
        place.local,
        place
            .iter_projections()
            .map(|(base, projection)| {
                let base_type = base.ty(&body.local_decls, cx.tcx).ty;
                let base_type = cx
                    .tcx
                    .try_normalize_erasing_regions(cx.typing_env(), base_type)
                    .unwrap_or(base_type);
                let sibling_fields_overlap = matches!(projection, ProjectionElem::Field(..))
                    && matches!(base_type.kind(), TyKind::Adt(adt, _) if adt.is_union());
                let imprecise_index = matches!(
                    projection,
                    ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. }
                );
                ResultProjectionKey {
                    display: format!("{projection:?}"),
                    sibling_fields_overlap,
                    imprecise_index,
                }
            })
            .collect(),
    )
}

fn rvs_is_exact_poll_ready_payload(place: &Place<'_>) -> bool {
    matches!(
        &place.projection[..],
        [ProjectionElem::Downcast(..), ProjectionElem::Field(..)]
    )
}

fn rvs_result_place_value(state: &ResultFlowState, place: &ResultPlaceKey) -> u8 {
    if place.1.is_empty() {
        state
            .values
            .get(&place.0)
            .copied()
            .unwrap_or(RVS_RESULT_UNKNOWN)
    } else {
        state
            .projected_values
            .get(place)
            .copied()
            .unwrap_or(RVS_RESULT_UNKNOWN)
    }
}

fn rvs_set_result_place_value_M(state: &mut ResultFlowState, place: &ResultPlaceKey, value: u8) {
    debug_assert_ne!(value, 0, "result state contains at least one possibility");
    if place.1.is_empty() {
        state.values.insert(place.0, value);
    } else {
        state.projected_values.insert(place.clone(), value);
    }
}

fn rvs_set_result_target_value_M(state: &mut ResultFlowState, target: &ResultPlaceKey, value: u8) {
    debug_assert_ne!(value, 0, "result state contains at least one possibility");
    if target.1.is_empty() {
        if state.values.contains_key(&target.0) {
            state.values.insert(target.0, value);
        }
        return;
    }
    let mut updated = false;
    for (place, projected_value) in &mut state.projected_values {
        if place.0 == target.0 && rvs_projection_paths_overlap(&place.1, &target.1) {
            *projected_value = value;
            updated = true;
        }
    }
    if !updated {
        state.projected_values.insert(target.clone(), value);
    }
}

fn rvs_projection_paths_overlap(
    left: &[ResultProjectionKey],
    right: &[ResultProjectionKey],
) -> bool {
    for (left, right) in left.iter().zip(right) {
        if left.display == right.display {
            continue;
        }
        if left.imprecise_index
            || right.imprecise_index
            || (left.sibling_fields_overlap && right.sibling_fields_overlap)
        {
            return true;
        }
        return false;
    }
    true
}

fn rvs_invalidate_relations_for_source_M(state: &mut ResultFlowState, source: &ResultPlaceKey) {
    state.discriminants.retain(|_, discriminant_source| {
        discriminant_source.source.0 != source.0
            || !rvs_projection_paths_overlap(&discriminant_source.source.1, &source.1)
    });
    state.predicates.retain(|_, (predicate_source, _)| {
        predicate_source.0 != source.0
            || !rvs_projection_paths_overlap(&predicate_source.1, &source.1)
    });
}

fn rvs_set_result_target_unknown_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &mut ResultFlowState,
    target: &ResultPlaceKey,
) {
    rvs_invalidate_relations_for_source_M(state, target);
    if target.1.is_empty() && state.values.contains_key(&target.0) {
        state.values.insert(
            target.0,
            rvs_unknown_result_value(cx, rvs_local_type(body, target.0)),
        );
    }
    for (place, value) in &mut state.projected_values {
        if place.0 == target.0 && rvs_projection_paths_overlap(&place.1, &target.1) {
            *value = RVS_RESULT_UNKNOWN;
        }
    }
}

fn rvs_call_preserves_result_variant(cx: &LateContext<'_>, def_id: DefId) -> bool {
    let path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    matches!(
        path.rvs_user_path().as_ref(),
        "core::result::Result::as_ref"
            | "core::result::Result::as_deref"
            | "core::result::Result::iter"
            | "core::result::Result::as_mut"
            | "core::result::Result::as_deref_mut"
            | "core::result::Result::iter_mut"
    )
}

fn rvs_call_is_result_from_output(path: &str) -> bool {
    matches!(
        path,
        "core::ops::try_trait::Try::from_output"
            | "core::result::from_output@core::ops::try_trait::Try"
    )
}

fn rvs_call_is_result_branch(path: &str) -> bool {
    matches!(
        path,
        "core::ops::try_trait::Try::branch" | "core::result::branch@core::ops::try_trait::Try"
    )
}

fn rvs_result_predicate_source(
    path: Option<&str>,
    references: &ResultReferenceMap,
) -> Option<(bool, ResultPlaceKey)> {
    let is_error_test = match path? {
        "core::result::Result::is_err" => true,
        "core::result::Result::is_ok" => false,
        _ => return None,
    };
    let mut sources = references
        .iter()
        .filter_map(|(source, reference)| reference.direct.then_some(source));
    let source = sources.next()?.clone();
    sources.next().is_none().then_some((is_error_test, source))
}

fn rvs_local_effect_summaries(
    cx: &LateContext<'_>,
    root_body: &rustc_middle::mir::Body<'_>,
) -> HashMap<DefId, Vec<LocalParamEffect>> {
    let mut owners = HashSet::new();
    let mut pending = rvs_local_callees(root_body)
        .into_iter()
        .collect::<VecDeque<_>>();
    while let Some(owner) = pending.pop_front() {
        if !owners.insert(owner) {
            continue;
        }
        if !cx.tcx.is_mir_available(owner.to_def_id()) {
            owners.remove(&owner);
            continue;
        }
        let body = cx.tcx.optimized_mir(owner);
        pending.extend(rvs_local_callees(body));
    }

    let mut summaries = owners
        .iter()
        .map(|owner| {
            let body = cx.tcx.optimized_mir(*owner);
            (
                owner.to_def_id(),
                vec![LocalParamEffect::default(); body.arg_count],
            )
        })
        .collect::<HashMap<_, _>>();
    loop {
        let mut changed = false;
        for owner in &owners {
            let body = cx.tcx.optimized_mir(*owner);
            let next = rvs_summarize_local_body(cx, body, &summaries);
            let entry = summaries
                .get_mut(&owner.to_def_id())
                .expect("never: every collected local body has a summary");
            if *entry != next {
                *entry = next;
                changed = true;
            }
        }
        if !changed {
            return summaries;
        }
    }
}

fn rvs_local_callees(body: &rustc_middle::mir::Body<'_>) -> HashSet<LocalDefId> {
    body.basic_blocks
        .iter()
        .filter_map(|block| match &block.terminator().kind {
            TerminatorKind::Call { func, .. } | TerminatorKind::TailCall { func, .. } => {
                rvs_operand_def_id(func).and_then(DefId::as_local)
            }
            _ => None,
        })
        .collect()
}

fn rvs_summarize_local_body<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    summaries: &HashMap<DefId, Vec<LocalParamEffect>>,
) -> Vec<LocalParamEffect> {
    body.args_iter()
        .map(|argument| rvs_summarize_local_parameter(cx, body, argument, summaries))
        .collect()
}

fn rvs_summarize_local_parameter<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    argument: Local,
    summaries: &HashMap<DefId, Vec<LocalParamEffect>>,
) -> LocalParamEffect {
    LocalParamEffect {
        direct: rvs_summarize_local_parameter_channel(
            cx,
            body,
            argument,
            summaries,
            ResultReference::rvs_direct_mutable(),
        ),
        nested: rvs_summarize_local_parameter_channel(
            cx,
            body,
            argument,
            summaries,
            ResultReference::rvs_nested_mutable(),
        ),
    }
}

fn rvs_summarize_local_parameter_channel<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    argument: Local,
    summaries: &HashMap<DefId, Vec<LocalParamEffect>>,
    initial_reference: ResultReference,
) -> LocalParamChannelEffect {
    let mut aliases = HashMap::from([(argument, initial_reference)]);
    let mut effect = LocalParamChannelEffect::default();
    loop {
        let previous_aliases = aliases.clone();
        let previous_effect = effect;
        for block in body.basic_blocks.iter() {
            for statement in &block.statements {
                match &statement.kind {
                    StatementKind::Assign(assignment) => {
                        let (destination, value) = &**assignment;
                        let source = rvs_summary_rvalue_reference(&aliases, value);
                        if rvs_place_dereferences_alias(destination, &aliases) {
                            effect.may_mutate = true;
                            effect.may_escape |= source.rvs_can_change_variant();
                        } else {
                            let source = if destination.projection.is_empty() {
                                source
                            } else {
                                source.rvs_nest()
                            };
                            rvs_merge_summary_alias_M(&mut aliases, destination.local, source);
                        }
                    }
                    StatementKind::SetDiscriminant { place, .. } => {
                        effect.may_mutate |= rvs_place_dereferences_alias(place, &aliases);
                    }
                    StatementKind::Intrinsic(intrinsic) => {
                        if let NonDivergingIntrinsic::CopyNonOverlapping(copy) = &**intrinsic {
                            effect.may_mutate |= rvs_summary_operand_reference(&aliases, &copy.dst)
                                .rvs_can_change_variant();
                        }
                    }
                    _ => {}
                }
            }
            rvs_summarize_terminator_M(
                cx,
                body,
                &block.terminator().kind,
                summaries,
                &mut aliases,
                &mut effect,
            );
        }
        if aliases == previous_aliases && effect == previous_effect {
            break;
        }
    }
    effect.may_return_alias |= aliases
        .get(&RETURN_PLACE)
        .copied()
        .is_some_and(ResultReference::rvs_has_reference);
    effect
}

fn rvs_summarize_terminator_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    terminator: &TerminatorKind<'tcx>,
    summaries: &HashMap<DefId, Vec<LocalParamEffect>>,
    aliases: &mut SummaryAliases,
    effect: &mut LocalParamChannelEffect,
) {
    match terminator {
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => {
            let called_def_id = rvs_operand_def_id(func);
            let argument_aliases = args
                .iter()
                .map(|argument| rvs_summary_operand_reference(aliases, &argument.node))
                .collect::<Vec<_>>();
            if let Some(callee_effects) = called_def_id.and_then(|def_id| summaries.get(&def_id)) {
                for (index, reference) in argument_aliases.iter().copied().enumerate() {
                    let callee_effect = callee_effects
                        .get(index)
                        .copied()
                        .unwrap_or_else(rvs_unknown_local_param_effect);
                    effect.may_mutate |= callee_effect.rvs_may_mutate(reference);
                    effect.may_escape |= callee_effect.rvs_may_escape(reference);
                    effect.may_drop |= callee_effect.rvs_may_drop(reference);
                    if callee_effect.rvs_may_return_alias(reference) {
                        rvs_merge_summary_alias_M(aliases, destination.local, reference);
                    }
                }
            } else {
                let preserves_arguments = called_def_id
                    .is_some_and(|def_id| rvs_call_preserves_argument_contents(cx, def_id));
                let preserves_result_variant = called_def_id
                    .is_some_and(|def_id| rvs_call_preserves_result_variant(cx, def_id));
                for reference in argument_aliases {
                    let can_mutate = reference.rvs_can_change_variant();
                    effect.may_mutate |= !preserves_arguments && can_mutate;
                    effect.may_escape |= !preserves_arguments && can_mutate;
                    let returned = if preserves_result_variant {
                        reference.rvs_preserve_direct_variant()
                    } else {
                        reference
                    };
                    rvs_merge_summary_alias_M(aliases, destination.local, returned);
                }
            }
        }
        TerminatorKind::TailCall { func, args, .. } => {
            let called_def_id = rvs_operand_def_id(func);
            let argument_aliases = args
                .iter()
                .map(|argument| rvs_summary_operand_reference(aliases, &argument.node))
                .collect::<Vec<_>>();
            if let Some(callee_effects) = called_def_id.and_then(|def_id| summaries.get(&def_id)) {
                for (index, reference) in argument_aliases.iter().copied().enumerate() {
                    let callee_effect = callee_effects
                        .get(index)
                        .copied()
                        .unwrap_or_else(rvs_unknown_local_param_effect);
                    effect.may_mutate |= callee_effect.rvs_may_mutate(reference);
                    effect.may_escape |= callee_effect.rvs_may_escape(reference);
                    effect.may_drop |= callee_effect.rvs_may_drop(reference);
                    effect.may_return_alias |= callee_effect.rvs_may_return_alias(reference);
                }
            } else {
                let preserves_arguments = called_def_id
                    .is_some_and(|def_id| rvs_call_preserves_argument_contents(cx, def_id));
                for reference in argument_aliases {
                    let can_mutate = reference.rvs_can_change_variant();
                    effect.may_mutate |= !preserves_arguments && can_mutate;
                    effect.may_escape |= !preserves_arguments && can_mutate;
                    effect.may_return_alias |= reference.rvs_has_reference();
                }
            }
        }
        TerminatorKind::Drop { place, .. } => {
            if rvs_summary_place_reference(aliases, place).rvs_can_change_variant() {
                let dropped_type = place.ty(&body.local_decls, cx.tcx).ty;
                let dropped_type = cx
                    .tcx
                    .try_normalize_erasing_regions(cx.typing_env(), dropped_type)
                    .unwrap_or(dropped_type);
                if matches!(dropped_type.kind(), TyKind::Param(_)) {
                    effect.may_drop = true;
                } else if dropped_type.needs_drop(cx.tcx, cx.typing_env()) {
                    effect.may_mutate = true;
                }
            }
        }
        TerminatorKind::InlineAsm { operands, .. } => {
            for operand in operands {
                let references = match operand {
                    InlineAsmOperand::In { value, .. } => {
                        rvs_summary_operand_reference(aliases, value)
                    }
                    InlineAsmOperand::InOut { in_value, .. } => {
                        rvs_summary_operand_reference(aliases, in_value)
                    }
                    InlineAsmOperand::Out { place, .. } => place
                        .as_ref()
                        .map_or_else(ResultReference::default, |place| {
                            rvs_summary_place_reference(aliases, place)
                        }),
                    _ => ResultReference::default(),
                };
                let can_mutate = references.rvs_can_change_variant();
                effect.may_mutate |= can_mutate;
                effect.may_escape |= can_mutate;
            }
        }
        _ => {}
    }
}

fn rvs_summary_rvalue_reference(aliases: &SummaryAliases, value: &Rvalue<'_>) -> ResultReference {
    match value {
        Rvalue::Ref(_, BorrowKind::Mut { .. }, place) | Rvalue::RawPtr(_, place) => {
            rvs_summary_place_reference(aliases, place)
        }
        Rvalue::Ref(_, _, place) => {
            rvs_summary_place_reference(aliases, place).rvs_preserve_direct_variant()
        }
        Rvalue::Use(operand)
        | Rvalue::Repeat(operand, _)
        | Rvalue::Cast(_, operand, _)
        | Rvalue::UnaryOp(_, operand) => rvs_summary_operand_reference(aliases, operand),
        Rvalue::CopyForDeref(place) => rvs_summary_place_reference(aliases, place),
        Rvalue::BinaryOp(_, operands) => {
            let (left, right) = &**operands;
            let mut merged = rvs_summary_operand_reference(aliases, left);
            merged.rvs_join_M(rvs_summary_operand_reference(aliases, right));
            merged
        }
        Rvalue::Aggregate(_, operands) => {
            let mut merged = ResultReference::default();
            for operand in operands {
                merged.rvs_join_M(rvs_summary_operand_reference(aliases, operand));
            }
            merged.rvs_nest()
        }
        _ => ResultReference::default(),
    }
}

fn rvs_summary_operand_reference(
    aliases: &SummaryAliases,
    operand: &Operand<'_>,
) -> ResultReference {
    rvs_operand_place(operand).map_or_else(ResultReference::default, |place| {
        rvs_summary_place_reference(aliases, place)
    })
}

fn rvs_summary_place_reference(aliases: &SummaryAliases, place: &Place<'_>) -> ResultReference {
    aliases.get(&place.local).copied().unwrap_or_default()
}

fn rvs_place_dereferences_alias(place: &Place<'_>, aliases: &SummaryAliases) -> bool {
    aliases
        .get(&place.local)
        .copied()
        .is_some_and(ResultReference::rvs_can_change_variant)
        && place
            .projection
            .iter()
            .any(|projection| matches!(projection, ProjectionElem::Deref))
}

fn rvs_merge_summary_alias_M(
    aliases: &mut SummaryAliases,
    destination: Local,
    reference: ResultReference,
) {
    if reference.rvs_has_reference() {
        aliases
            .entry(destination)
            .and_modify(|existing| existing.rvs_join_M(reference))
            .or_insert(reference);
    }
}

fn rvs_call_preserves_argument_contents(cx: &LateContext<'_>, def_id: DefId) -> bool {
    let path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    matches!(path.rvs_user_path().as_ref(), "core::hint::black_box")
        || rvs_call_preserves_result_variant(cx, def_id)
        || rvs_call_is_result_branch(path.rvs_user_path().as_ref())
}

fn rvs_call_is_identity(cx: &LateContext<'_>, def_id: DefId) -> bool {
    let path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    matches!(path.rvs_user_path().as_ref(), "core::hint::black_box")
}

fn rvs_call_is_future_poll(cx: &LateContext<'_>, def_id: DefId) -> bool {
    let path = DefPath::rvs_new(rvs_def_path(cx, def_id));
    let path = path.rvs_user_path();
    path == "core::future::future::Future::poll"
        || path.ends_with("poll@core::future::future::Future")
}

fn rvs_refine_switch_edge_M<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &mut ResultFlowState,
    discriminant_source: Option<ResultDiscriminant>,
    predicate: Option<(ResultPlaceKey, bool)>,
    value: u128,
) -> bool {
    debug_assert!(
        value <= 1 || (discriminant_source.is_none() && predicate.is_none()),
        "tracked Result and boolean switches use binary discriminants"
    );
    if discriminant_source.is_some() {
        return rvs_refine_result_discriminant_M(cx, body, state, discriminant_source, value);
    }
    let Some((source, is_error_test)) = predicate else {
        return true;
    };
    let variant = match (is_error_test, value) {
        (true, 0) | (false, 1) => RVS_RESULT_OK,
        (true, 1) | (false, 0) => RVS_RESULT_ERR,
        _ => return false,
    };
    let current = rvs_result_place_value(state, &source);
    if current & variant == 0 {
        return false;
    }
    rvs_set_result_place_value_M(state, &source, current & variant);
    true
}

fn rvs_refine_result_discriminant_M<'tcx>(
    _cx: &LateContext<'tcx>,
    _body: &rustc_middle::mir::Body<'tcx>,
    state: &mut ResultFlowState,
    source: Option<ResultDiscriminant>,
    discriminant: u128,
) -> bool {
    debug_assert!(
        discriminant <= 1 || source.is_none(),
        "Result and Poll use binary discriminants"
    );
    let Some(source) = source else {
        return true;
    };
    let variant = if source.is_poll {
        match discriminant {
            0 => RVS_RESULT_OK | RVS_RESULT_ERR,
            1 => RVS_RESULT_PENDING,
            _ => return false,
        }
    } else {
        match discriminant {
            0 => RVS_RESULT_OK,
            1 => RVS_RESULT_ERR,
            _ => return false,
        }
    };
    let current = rvs_result_place_value(state, &source.source);
    if current & variant == 0 {
        return false;
    }
    rvs_set_result_place_value_M(state, &source.source, current & variant);
    true
}

fn rvs_result_variant_value<'tcx>(
    cx: &LateContext<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    state: &ResultFlowState,
    result_type: Ty<'tcx>,
    variant_index: rustc_abi::VariantIdx,
    first_operand: Option<&Operand<'tcx>>,
) -> Option<u8> {
    let result_type = cx
        .tcx
        .try_normalize_erasing_regions(cx.typing_env(), result_type)
        .unwrap_or(result_type);
    let TyKind::Adt(adt, arguments) = result_type.kind() else {
        return None;
    };
    if cx.tcx.is_diagnostic_item(sym::Result, adt.did()) {
        return match adt.variant(variant_index).name.as_str() {
            "Ok" => Some(RVS_RESULT_OK),
            "Err" => Some(RVS_RESULT_ERR),
            _ => None,
        };
    }
    if cx.tcx.lang_items().poll() == Some(adt.did())
        && rvs_result_error_type(cx, arguments.type_at(0)).is_some()
    {
        return match adt.variant(variant_index).name.as_str() {
            "Ready" => {
                first_operand.map(|operand| rvs_operand_result_value(cx, body, state, operand))
            }
            "Pending" => Some(RVS_RESULT_PENDING),
            _ => None,
        };
    }
    None
}

fn rvs_unknown_result_value<'tcx>(cx: &LateContext<'tcx>, result_type: Ty<'tcx>) -> u8 {
    rvs_result_error_type(cx, result_type).map_or(RVS_RESULT_UNKNOWN, |error| {
        let pending = if rvs_poll_result_type(cx, result_type).is_some() {
            RVS_RESULT_PENDING
        } else {
            0
        };
        if error.is_privately_uninhabited(cx.tcx, cx.typing_env()) {
            RVS_RESULT_OK | pending
        } else {
            RVS_RESULT_UNKNOWN | pending
        }
    })
}

fn rvs_result_error_type<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>) -> Option<Ty<'tcx>> {
    let ty = cx
        .tcx
        .try_normalize_erasing_regions(cx.typing_env(), ty)
        .unwrap_or(ty);
    let TyKind::Adt(adt, arguments) = ty.kind() else {
        return None;
    };
    if cx.tcx.is_diagnostic_item(sym::Result, adt.did()) {
        return Some(arguments.type_at(1));
    }
    rvs_poll_result_type(cx, ty).and_then(|result| rvs_result_error_type(cx, result))
}

fn rvs_local_type<'tcx>(body: &rustc_middle::mir::Body<'tcx>, local: Local) -> Ty<'tcx> {
    body.local_decls
        .get(local)
        .expect("never: MIR local belongs to this body")
        .ty
}

fn rvs_poll_result_type<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>) -> Option<Ty<'tcx>> {
    let ty = cx
        .tcx
        .try_normalize_erasing_regions(cx.typing_env(), ty)
        .unwrap_or(ty);
    let TyKind::Adt(adt, arguments) = ty.kind() else {
        return None;
    };
    (cx.tcx.lang_items().poll() == Some(adt.did())).then(|| arguments.type_at(0))
}
