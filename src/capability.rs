use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use crate::symbols::rvs_function_name_segment;

use rustc_hir::{self, Safety};
use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// 能力之八德：异步、阻塞、读写、可变、端口、副作用、线程、不安。
/// 八德既立，函数之名即为契约，调用之际便有章法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capability {
    A,
    B,
    I,
    M,
    P,
    S,
    T,
    U,
}

impl Capability {
    /// 从后缀字母解析出对应的 Capability。未知字符返回 None。
    pub fn rvs_from_char(c: char) -> Option<Self> {
        match c {
            'A' => Some(Self::A),
            'B' => Some(Self::B),
            'I' => Some(Self::I),
            'M' => Some(Self::M),
            'P' => Some(Self::P),
            'S' => Some(Self::S),
            'T' => Some(Self::T),
            'U' => Some(Self::U),
            _ => None,
        }
    }

    /// 返回能力对应的大写后缀字母。
    pub fn rvs_as_char(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
            Self::I => 'I',
            Self::M => 'M',
            Self::P => 'P',
            Self::S => 'S',
            Self::T => 'T',
            Self::U => 'U',
        }
    }

    /// 返回能力的英文语义名（用于报告显示）。
    pub fn rvs_description(self) -> &'static str {
        match self {
            Self::A => "Async",
            Self::B => "Blocking",
            Self::I => "IO",
            Self::M => "Mutable",
            Self::P => "Port",
            Self::S => "SideEffect",
            Self::T => "ThreadLocal",
            Self::U => "Unsafe",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.rvs_as_char(), self.rvs_description())
    }
}

#[cfg(test)]
const VALID_SUFFIX_CHARS: &[char] = &['A', 'B', 'I', 'M', 'P', 'S', 'T', 'U'];

/// Canonical semantic view of a function name or def-path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedFunctionName<'a> {
    segment: &'a str,
    base_name: &'a str,
    has_rvs_prefix: bool,
    raw_suffix: Option<&'a str>,
    known_caps: CapabilitySet,
    unknown_suffix_letters: Vec<char>,
    duplicate_suffix_letters: Vec<char>,
    suffix_is_canonical: bool,
}

impl<'a> ParsedFunctionName<'a> {
    /// Parse a bare function name or full def-path once into all naming facts.
    pub(crate) fn rvs_parse(name: &'a str) -> Self {
        let segment = rvs_function_name_segment(name);
        let Some(rest) = segment.strip_prefix("rvs_") else {
            return Self {
                segment,
                base_name: segment,
                has_rvs_prefix: false,
                raw_suffix: None,
                known_caps: CapabilitySet::rvs_new(),
                unknown_suffix_letters: Vec::new(),
                duplicate_suffix_letters: Vec::new(),
                suffix_is_canonical: true,
            };
        };

        let (base_name, raw_suffix) = rest.rfind('_').map_or((rest, None), |pos| {
            let potential_suffix = rest.get(pos + 1..).unwrap_or("");
            if !potential_suffix.is_empty()
                && potential_suffix.chars().all(|c| c.is_ascii_uppercase())
            {
                (rest.get(..pos).unwrap_or(""), Some(potential_suffix))
            } else {
                (rest, None)
            }
        });
        let known_caps = raw_suffix
            .map(CapabilitySet::rvs_from_str_allow_unknown)
            .unwrap_or_else(CapabilitySet::rvs_new);
        let mut seen = BTreeSet::new();
        let mut seen_unknown = BTreeSet::new();
        let mut seen_duplicates = BTreeSet::new();
        let mut unknown_suffix_letters = Vec::new();
        let mut duplicate_suffix_letters = Vec::new();
        for letter in raw_suffix.unwrap_or("").chars() {
            if !seen.insert(letter) && seen_duplicates.insert(letter) {
                duplicate_suffix_letters.push(letter);
            }
            if Capability::rvs_from_char(letter).is_none() && seen_unknown.insert(letter) {
                unknown_suffix_letters.push(letter);
            }
        }
        let suffix_is_canonical = raw_suffix.is_none_or(|suffix| {
            suffix
                .as_bytes()
                .windows(2)
                .all(|letters| matches!(letters, [left, right] if left <= right))
        });

        Self {
            segment,
            base_name,
            has_rvs_prefix: true,
            raw_suffix,
            known_caps,
            unknown_suffix_letters,
            duplicate_suffix_letters,
            suffix_is_canonical,
        }
    }

    pub(crate) fn rvs_base_name(&self) -> &'a str {
        self.base_name
    }

    pub(crate) fn rvs_has_rvs_prefix(&self) -> bool {
        self.has_rvs_prefix
    }

    pub(crate) fn rvs_raw_suffix(&self) -> Option<&'a str> {
        self.raw_suffix
    }

    pub(crate) fn rvs_known_caps(&self) -> &CapabilitySet {
        &self.known_caps
    }

    pub(crate) fn rvs_unknown_suffix_letters(&self) -> &[char] {
        &self.unknown_suffix_letters
    }

    pub(crate) fn rvs_duplicate_suffix_letters(&self) -> &[char] {
        &self.duplicate_suffix_letters
    }

    pub(crate) fn rvs_suffix_is_canonical(&self) -> bool {
        self.suffix_is_canonical
    }

    pub(crate) fn rvs_canonical_suffix(&self) -> Option<String> {
        self.raw_suffix.map(|suffix| {
            let mut letters: Vec<char> = suffix.chars().collect();
            letters.sort_unstable();
            letters.into_iter().collect()
        })
    }

    /// Preserve the historical rule that an unknown-only suffix is undeclared.
    pub(crate) fn rvs_declared_caps(&self) -> Option<CapabilitySet> {
        if !self.has_rvs_prefix
            || (!self.unknown_suffix_letters.is_empty() && self.known_caps.rvs_is_empty())
        {
            return None;
        }
        Some(self.known_caps.clone())
    }
}

/// 一组能力，如同一面旗——旗上画的，便是这函数的本事。
/// 旗上没画的，便是它干不了的。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl Serialize for CapabilitySet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.rvs_letters())
    }
}

impl<'de> Deserialize<'de> for CapabilitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::rvs_from_str(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityCompleteness {
    Complete,
    Incomplete,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub(crate) enum CapabilityBasis {
    Explicit,
    Inferred,
    TraitVote {
        implementations: usize,
        threshold: usize,
        votes: BTreeMap<Capability, usize>,
    },
    Port,
}

impl CapabilityBasis {
    pub(crate) fn rvs_name(&self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Inferred => "inferred",
            Self::TraitVote { .. } => "trait_vote",
            Self::Port => "port",
        }
    }
}

impl CapabilityCompleteness {
    pub(crate) fn rvs_name(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Incomplete => "incomplete",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapabilitySource {
    pub(crate) layer: String,
    pub(crate) file: PathBuf,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CapabilityInfo {
    caps: CapabilitySet,
    basis: CapabilityBasis,
    completeness: CapabilityCompleteness,
    #[serde(skip)]
    source: Option<CapabilitySource>,
}

#[derive(Debug, Snafu, PartialEq, Eq)]
pub(crate) enum CapabilityKnowledgeError {
    #[snafu(display("basis '{basis}' requires completeness 'complete', got '{completeness}'"))]
    CompleteBasis {
        basis: &'static str,
        completeness: &'static str,
    },
    #[snafu(display("Port knowledge must contain exactly P, got '{caps}'"))]
    PortCaps { caps: String },
    #[snafu(display("trait vote must contain at least one implementation"))]
    TraitVoteWithoutImplementations,
    #[snafu(display(
        "trait vote threshold must be {expected} for {implementations} implementations, got {threshold}"
    ))]
    TraitVoteThreshold {
        implementations: usize,
        threshold: usize,
        expected: usize,
    },
    #[snafu(display("trait vote completeness cannot be 'unknown'"))]
    TraitVoteUnknownCompleteness,
    #[snafu(display("trait vote capability {capability:?} is not propagated"))]
    TraitVoteNonPropagatedCapability { capability: Capability },
    #[snafu(display("trait vote count for {capability:?} must be positive"))]
    TraitVoteZeroCount { capability: Capability },
    #[snafu(display(
        "trait vote count for {capability:?} exceeds implementation count: {count} > {implementations}"
    ))]
    TraitVoteCountExceedsImplementations {
        capability: Capability,
        count: usize,
        implementations: usize,
    },
    #[snafu(display("trait vote selected caps '{actual}' do not match vote result '{expected}'"))]
    TraitVoteSelectedCaps { actual: String, expected: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValidatedCapabilityKnowledge;

impl CapabilityInfo {
    pub(crate) fn rvs_new(
        caps: CapabilitySet,
        basis: CapabilityBasis,
        completeness: CapabilityCompleteness,
    ) -> Self {
        Self {
            caps,
            basis,
            completeness,
            source: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn rvs_explicit(caps: CapabilitySet) -> Self {
        Self::rvs_new(
            caps,
            CapabilityBasis::Explicit,
            CapabilityCompleteness::Complete,
        )
    }

    pub(crate) fn rvs_inferred(caps: CapabilitySet) -> Self {
        Self::rvs_new(
            caps,
            CapabilityBasis::Inferred,
            CapabilityCompleteness::Complete,
        )
    }

    pub(crate) fn rvs_trait_vote(
        caps: CapabilitySet,
        implementations: usize,
        threshold: usize,
        votes: BTreeMap<Capability, usize>,
        completeness: CapabilityCompleteness,
    ) -> Self {
        debug_assert!(implementations > 0, "trait vote has implementations");
        debug_assert!(threshold > 0, "trait vote threshold is positive");
        debug_assert!(
            threshold <= implementations,
            "trait vote threshold does not exceed implementations"
        );
        Self::rvs_new(
            caps,
            CapabilityBasis::TraitVote {
                implementations,
                threshold,
                votes,
            },
            completeness,
        )
    }

    pub(crate) fn rvs_caps(&self) -> &CapabilitySet {
        &self.caps
    }

    pub(crate) fn rvs_basis(&self) -> &CapabilityBasis {
        &self.basis
    }

    pub(crate) fn rvs_completeness(&self) -> CapabilityCompleteness {
        self.completeness
    }

    pub(crate) fn rvs_source(&self) -> Option<&CapabilitySource> {
        self.source.as_ref()
    }

    pub(crate) fn rvs_with_source_M(&mut self, source: CapabilitySource) {
        debug_assert!(source.line > 0, "capability source line is one-based");
        self.source = Some(source);
    }

    pub(crate) fn rvs_check_invariants(
        &self,
    ) -> Result<ValidatedCapabilityKnowledge, CapabilityKnowledgeError> {
        match &self.basis {
            CapabilityBasis::Explicit => {
                if self.completeness != CapabilityCompleteness::Complete {
                    return Err(CapabilityKnowledgeError::CompleteBasis {
                        basis: self.basis.rvs_name(),
                        completeness: self.completeness.rvs_name(),
                    });
                }
            }
            CapabilityBasis::Inferred => {}
            CapabilityBasis::Port => {
                if self.completeness != CapabilityCompleteness::Complete {
                    return Err(CapabilityKnowledgeError::CompleteBasis {
                        basis: self.basis.rvs_name(),
                        completeness: self.completeness.rvs_name(),
                    });
                }
                if self.caps != CapabilityPolicy::rvs_port_method_caps() {
                    return Err(CapabilityKnowledgeError::PortCaps {
                        caps: self.caps.rvs_letters(),
                    });
                }
            }
            CapabilityBasis::TraitVote {
                implementations,
                threshold,
                votes,
            } => {
                if *implementations == 0 {
                    return Err(CapabilityKnowledgeError::TraitVoteWithoutImplementations);
                }
                let expected_threshold = implementations.div_ceil(2);
                if *threshold != expected_threshold {
                    return Err(CapabilityKnowledgeError::TraitVoteThreshold {
                        implementations: *implementations,
                        threshold: *threshold,
                        expected: expected_threshold,
                    });
                }
                if self.completeness == CapabilityCompleteness::Unknown {
                    return Err(CapabilityKnowledgeError::TraitVoteUnknownCompleteness);
                }
                let mut selected = CapabilitySet::rvs_new();
                for (capability, count) in votes {
                    if !CapabilityPolicy::rvs_is_propagated_cap(*capability) {
                        return Err(CapabilityKnowledgeError::TraitVoteNonPropagatedCapability {
                            capability: *capability,
                        });
                    }
                    if *count == 0 {
                        return Err(CapabilityKnowledgeError::TraitVoteZeroCount {
                            capability: *capability,
                        });
                    }
                    if *count > *implementations {
                        return Err(
                            CapabilityKnowledgeError::TraitVoteCountExceedsImplementations {
                                capability: *capability,
                                count: *count,
                                implementations: *implementations,
                            },
                        );
                    }
                    if *count >= *threshold {
                        selected.rvs_insert_M(*capability);
                    }
                }
                if self.caps != selected {
                    return Err(CapabilityKnowledgeError::TraitVoteSelectedCaps {
                        actual: self.caps.rvs_letters(),
                        expected: selected.rvs_letters(),
                    });
                }
            }
        }
        Ok(ValidatedCapabilityKnowledge)
    }
}

/// Facts observed from a function signature/body before policy is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityFacts {
    pub has_async: bool,
    pub is_unsafe_fn: bool,
    pub has_mut_param: bool,
    pub has_static_ref: bool,
    pub has_static_mut_ref: bool,
    pub has_thread_local_ref: bool,
    pub is_port_method: bool,
}

impl CapabilityFacts {
    /// Merge observations from another compilation of the same function.
    pub fn rvs_merge_M(&mut self, other: Self) {
        self.has_async |= other.has_async;
        self.is_unsafe_fn |= other.is_unsafe_fn;
        self.has_mut_param |= other.has_mut_param;
        self.has_static_ref |= other.has_static_ref;
        self.has_static_mut_ref |= other.has_static_mut_ref;
        self.has_thread_local_ref |= other.has_thread_local_ref;
        self.is_port_method |= other.is_port_method;
    }

    /// Build capability facts from a function signature and precomputed mutability.
    pub fn rvs_from_signature(
        sig: &rustc_hir::FnSig<'_>,
        has_mut_param: bool,
        is_port_method: bool,
    ) -> Self {
        Self {
            has_async: sig.header.asyncness.is_async(),
            is_unsafe_fn: matches!(
                sig.header.safety,
                rustc_hir::HeaderSafety::Normal(Safety::Unsafe)
            ),
            has_mut_param,
            has_static_ref: false,
            has_static_mut_ref: false,
            has_thread_local_ref: false,
            is_port_method,
        }
    }

    /// Attach static/thread-local observations collected from the function body.
    pub fn rvs_with_static_refs(
        mut self,
        has_static_ref: bool,
        has_static_mut_ref: bool,
        has_thread_local_ref: bool,
    ) -> Self {
        self.has_static_ref = has_static_ref;
        self.has_static_mut_ref = has_static_mut_ref;
        self.has_thread_local_ref = has_thread_local_ref;
        self
    }
}

/// Central policy for deriving capability sets from observed facts.
#[derive(Debug)]
pub struct CapabilityPolicy;

impl CapabilityPolicy {
    /// Return capabilities propagated from callees to callers in canonical order.
    pub const fn rvs_propagated_caps() -> [Capability; 5] {
        [
            Capability::B,
            Capability::I,
            Capability::P,
            Capability::S,
            Capability::T,
        ]
    }

    /// Return the capability that every World Port operation has by structure.
    pub fn rvs_port_method_caps() -> CapabilitySet {
        let mut caps = CapabilitySet::rvs_new();
        caps.rvs_insert_M(Capability::P);
        caps
    }

    /// Infer initial capabilities from function facts before call propagation.
    pub fn rvs_signature_caps(facts: CapabilityFacts) -> CapabilitySet {
        let mut caps = CapabilitySet::rvs_new();
        if facts.is_port_method {
            caps.rvs_insert_M(Capability::P);
        }
        if facts.has_async {
            caps.rvs_insert_M(Capability::A);
        }
        if facts.is_unsafe_fn {
            caps.rvs_insert_M(Capability::U);
        }
        if facts.has_mut_param {
            caps.rvs_insert_M(Capability::M);
        }
        let _ = caps.rvs_extend_filtered_M(&Self::rvs_static_caps(facts), |_| true);
        caps
    }

    /// Infer the operation-owned portion of a World Port contract.
    ///
    /// Body observations are checked against the voted contract rather than
    /// defining the contract they are meant to satisfy.
    pub fn rvs_port_operation_signature_caps(mut facts: CapabilityFacts) -> CapabilitySet {
        debug_assert!(
            facts.is_port_method,
            "operation must belong to a World Port"
        );
        facts.has_static_ref = false;
        facts.has_static_mut_ref = false;
        facts.has_thread_local_ref = false;
        Self::rvs_signature_caps(facts)
    }

    /// Return capabilities required by static and thread-local observations.
    pub fn rvs_static_caps(facts: CapabilityFacts) -> CapabilitySet {
        let mut caps = CapabilitySet::rvs_new();
        if facts.has_static_mut_ref {
            caps.rvs_insert_M(Capability::S);
            caps.rvs_insert_M(Capability::U);
        } else if facts.has_static_ref {
            caps.rvs_insert_M(Capability::S);
        }
        if facts.has_thread_local_ref {
            caps.rvs_insert_M(Capability::S);
            caps.rvs_insert_M(Capability::T);
        }
        caps
    }

    /// Aggregate propagated capabilities selected by an at-least-half vote.
    pub(crate) fn rvs_at_least_half_vote<'a>(
        implementations: impl IntoIterator<Item = &'a CapabilitySet>,
    ) -> Option<(CapabilitySet, usize, BTreeMap<Capability, usize>)> {
        let implementations: Vec<&CapabilitySet> = implementations.into_iter().collect();
        if implementations.is_empty() {
            return None;
        }
        let threshold = implementations.len().div_ceil(2);
        let mut counts = BTreeMap::new();
        for caps in &implementations {
            for capability in caps
                .rvs_iter()
                .filter(|capability| Self::rvs_is_propagated_cap(*capability))
            {
                *counts.entry(capability).or_default() += 1;
            }
        }
        let mut selected = CapabilitySet::rvs_new();
        for (capability, count) in &counts {
            if *count >= threshold {
                selected.rvs_insert_M(*capability);
            }
        }
        Some((selected, threshold, counts))
    }

    /// Return whether a capability propagates from callees to callers.
    pub fn rvs_is_propagated_cap(cap: Capability) -> bool {
        Self::rvs_propagated_caps().contains(&cap)
    }

    /// Return the capabilities required across a call edge.
    ///
    /// A callee containing `P` is reached through a Port boundary, so its
    /// concrete implementation effects do not propagate to the caller.
    pub fn rvs_call_edge_caps(callee: &CapabilitySet) -> CapabilitySet {
        if callee.rvs_contains(Capability::P) {
            return Self::rvs_port_method_caps();
        }
        let mut propagated = CapabilitySet::rvs_new();
        let _ = propagated.rvs_extend_filtered_M(callee, Self::rvs_is_propagated_cap);
        propagated
    }

    /// Return whether signature inference requires a suffix capability.
    #[cfg(test)]
    pub fn rvs_requires_signature_cap(facts: CapabilityFacts, cap: Capability) -> bool {
        Self::rvs_signature_caps(facts).rvs_contains(cap)
    }

    /// Return the capability set allowed for good functions.
    pub fn rvs_good_caps() -> CapabilitySet {
        CapabilitySet(
            [Capability::A, Capability::B, Capability::M]
                .into_iter()
                .collect(),
        )
    }

    /// Return the capability set allowed for ok functions.
    pub fn rvs_ok_caps() -> CapabilitySet {
        CapabilitySet(
            [Capability::A, Capability::B, Capability::M, Capability::P]
                .into_iter()
                .collect(),
        )
    }

    /// Return whether a capability set is good.
    pub fn rvs_is_good(caps: &CapabilitySet) -> bool {
        caps.rvs_is_subset_of(&Self::rvs_good_caps())
    }

    /// Return whether a capability set is ok.
    pub fn rvs_is_ok(caps: &CapabilitySet) -> bool {
        caps.rvs_is_subset_of(&Self::rvs_ok_caps())
    }

    /// Return whether `caller` is allowed to call `callee`.
    pub fn rvs_can_call(caller: &CapabilitySet, callee: &CapabilitySet) -> bool {
        Self::rvs_call_edge_caps(callee)
            .0
            .iter()
            .all(|cap| caller.0.contains(cap))
    }

    /// Return the capabilities missing from `caller` when calling `callee`.
    pub fn rvs_missing_for(caller: &CapabilitySet, callee: &CapabilitySet) -> BTreeSet<Capability> {
        Self::rvs_call_edge_caps(callee)
            .0
            .into_iter()
            .filter(|cap| !caller.0.contains(cap))
            .collect()
    }
}

impl CapabilitySet {
    /// 构造一个空的能力集。
    pub fn rvs_new() -> Self {
        Self(BTreeSet::new())
    }

    /// 从后缀字符串解析能力集。遇到非法字母返回错误。
    pub fn rvs_from_str(s: &str) -> Result<Self, CapabilityParseError> {
        let mut set = BTreeSet::new();
        for c in s.chars() {
            let cap = Capability::rvs_from_char(c)
                .ok_or(CapabilityParseError::InvalidLetter { letter: c })?;
            if !set.insert(cap) {
                return Err(CapabilityParseError::DuplicateLetter { letter: c });
            }
        }
        Ok(Self(set))
    }

    /// 从已经校验过的后缀字符串解析能力集（预期任何字母都合法）。
    #[cfg(test)]
    pub fn rvs_from_validated(s: &str) -> Self {
        let mut set = BTreeSet::new();
        for c in s.chars() {
            let cap = match c {
                'A' => Capability::A,
                'B' => Capability::B,
                'I' => Capability::I,
                'M' => Capability::M,
                'P' => Capability::P,
                'S' => Capability::S,
                'T' => Capability::T,
                'U' => Capability::U,
                _ => {
                    debug_assert!(false, "后缀已验，字符必合法");
                    continue;
                }
            };
            set.insert(cap);
        }
        Self(set)
    }

    /// 从后缀字符串中萃取已知能力字母，忽略未知字母。
    /// 用于处理后缀含非标准字母（如 E）的情况。
    fn rvs_from_str_allow_unknown(suffix: &str) -> Self {
        let mut set = BTreeSet::new();
        for c in suffix.chars() {
            if let Some(cap) = Capability::rvs_from_char(c) {
                set.insert(cap);
            }
        }
        Self(set)
    }

    /// 我的能力是否全在你允许的范围之内。
    pub fn rvs_is_subset_of(&self, allowed: &Self) -> bool {
        self.0.iter().all(|cap| allowed.0.contains(cap))
    }

    /// 判断能力集是否为空。
    pub fn rvs_is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 判断能力集是否包含某项能力。
    pub fn rvs_contains(&self, cap: Capability) -> bool {
        self.0.contains(&cap)
    }

    /// 遍历能力集中的所有能力。
    pub fn rvs_iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    /// Render capabilities as their canonical ordered suffix letters.
    pub fn rvs_letters(&self) -> String {
        self.rvs_iter().map(Capability::rvs_as_char).collect()
    }

    /// Render the canonical descriptions used in capsmap comments.
    pub fn rvs_descriptions(&self) -> String {
        self.rvs_iter()
            .map(Capability::rvs_description)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Extend this set with selected capabilities, returning whether it changed.
    pub fn rvs_extend_filtered_M(
        &mut self,
        other: &Self,
        include: impl Fn(Capability) -> bool,
    ) -> bool {
        let old_len = self.0.len();
        self.0.extend(other.rvs_iter().filter(|cap| include(*cap)));
        self.0.len() != old_len
    }

    /// 返回能力集中能力的个数。
    #[cfg(test)]
    pub fn rvs_len(&self) -> usize {
        self.0.len()
    }

    /// 向能力集中插入一项能力。
    pub fn rvs_insert_M(&mut self, cap: Capability) {
        self.0.insert(cap);
    }
}

impl fmt::Display for CapabilitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let caps: Vec<String> = self.0.iter().map(|c| c.rvs_as_char().to_string()).collect();
        write!(f, "{{{}}}", caps.join(", "))
    }
}

#[derive(Debug, Snafu)]
pub enum CapabilityParseError {
    #[snafu(display("invalid capability letter: '{letter}'"))]
    InvalidLetter { letter: char },
    #[snafu(display("duplicate capability letter: '{letter}'"))]
    DuplicateLetter { letter: char },
}

/// 拆解 rvs_ 函数之名，得其骨（基名）与其魂（能力集）。
///
/// 拆法：取末段下划线之后的部分，若尽是能力字母，则视为后缀；
/// 否则，全名即基名，能力为空。
///
/// 亦能处理路径限定之名，如 `CapsMap::rvs_parse`，
/// 取末段路径片段而拆之。
///
/// 例：rvs_write_db_AIS     → 基名 write_db，能力 {A, I, S}
/// 例：rvs_add               → 基名 add，能力 {}
/// 例：CapsMap::rvs_parse  → 基名 parse，能力 {}
pub fn rvs_parse_function(name: &str) -> Option<(&str, CapabilitySet)> {
    let parsed = ParsedFunctionName::rvs_parse(name);
    if !parsed.rvs_has_rvs_prefix() {
        return None;
    }
    Some((parsed.rvs_base_name(), parsed.rvs_known_caps().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{rvs_register_test_coverage, rvs_snapshot_BIS};

    fn rvs_caps_letters(caps: &CapabilitySet) -> String {
        caps.rvs_iter().map(|cap| cap.rvs_as_char()).collect()
    }

    #[test]
    fn test_20260716_capability_knowledge_completeness_matrix() {
        let cases = [
            (
                "explicit_complete",
                "B",
                CapabilityBasis::Explicit,
                CapabilityCompleteness::Complete,
                true,
            ),
            (
                "explicit_incomplete",
                "B",
                CapabilityBasis::Explicit,
                CapabilityCompleteness::Incomplete,
                false,
            ),
            (
                "explicit_unknown",
                "B",
                CapabilityBasis::Explicit,
                CapabilityCompleteness::Unknown,
                false,
            ),
            (
                "inferred_complete",
                "B",
                CapabilityBasis::Inferred,
                CapabilityCompleteness::Complete,
                true,
            ),
            (
                "inferred_incomplete",
                "B",
                CapabilityBasis::Inferred,
                CapabilityCompleteness::Incomplete,
                true,
            ),
            (
                "inferred_unknown",
                "B",
                CapabilityBasis::Inferred,
                CapabilityCompleteness::Unknown,
                true,
            ),
            (
                "port_complete",
                "P",
                CapabilityBasis::Port,
                CapabilityCompleteness::Complete,
                true,
            ),
            (
                "port_incomplete",
                "P",
                CapabilityBasis::Port,
                CapabilityCompleteness::Incomplete,
                false,
            ),
            (
                "trait_vote_complete",
                "S",
                CapabilityBasis::TraitVote {
                    implementations: 2,
                    threshold: 1,
                    votes: BTreeMap::from([(Capability::S, 1)]),
                },
                CapabilityCompleteness::Complete,
                true,
            ),
            (
                "trait_vote_incomplete",
                "S",
                CapabilityBasis::TraitVote {
                    implementations: 2,
                    threshold: 1,
                    votes: BTreeMap::from([(Capability::S, 1)]),
                },
                CapabilityCompleteness::Incomplete,
                true,
            ),
            (
                "trait_vote_unknown",
                "S",
                CapabilityBasis::TraitVote {
                    implementations: 2,
                    threshold: 1,
                    votes: BTreeMap::from([(Capability::S, 1)]),
                },
                CapabilityCompleteness::Unknown,
                false,
            ),
        ];
        let mut output = String::new();
        for (name, caps, basis, completeness, expected_valid) in cases {
            let info = CapabilityInfo::rvs_new(
                CapabilitySet::rvs_from_validated(caps),
                basis,
                completeness,
            );
            let result = info.rvs_check_invariants();
            output.push_str(&format!(
                "{name}: {}\n",
                result
                    .as_ref()
                    .map_or_else(|error| format!("error={error}"), |_| "ok".to_string())
            ));
            assert_eq!(result.is_ok(), expected_valid, "{name}");
            if name == "explicit_incomplete" {
                assert!(matches!(
                    result,
                    Err(CapabilityKnowledgeError::CompleteBasis { .. })
                ));
            }
        }
        rvs_snapshot_BIS(
            "test_20260716_capability_knowledge_completeness_matrix",
            &output,
        );
    }

    #[test]
    fn test_20260709_capability_metadata_table() {
        let valid = [
            ('A', Capability::A, "Async"),
            ('B', Capability::B, "Blocking"),
            ('I', Capability::I, "IO"),
            ('M', Capability::M, "Mutable"),
            ('P', Capability::P, "Port"),
            ('S', Capability::S, "SideEffect"),
            ('T', Capability::T, "ThreadLocal"),
            ('U', Capability::U, "Unsafe"),
        ];
        let mut output = String::new();
        for (letter, cap, description) in valid {
            output.push_str(&format!(
                "{letter}: description={description} display={cap}\n"
            ));
            assert_eq!(Capability::rvs_from_char(letter), Some(cap), "{letter}");
            assert_eq!(cap.rvs_as_char(), letter, "{letter}");
            assert_eq!(cap.rvs_description(), description, "{letter}");
            assert_eq!(format!("{cap}"), format!("{letter}({description})"));
        }
        for letter in ['X', 'a', '1', '_'] {
            output.push_str(&format!("{letter}: invalid\n"));
            assert_eq!(Capability::rvs_from_char(letter), None, "{letter}");
        }
        for letter in VALID_SUFFIX_CHARS.iter().copied() {
            let cap = Capability::rvs_from_char(letter).unwrap();
            assert_eq!(cap.rvs_as_char(), letter);
        }
        rvs_snapshot_BIS("test_20260709_capability_metadata_table", &output);
    }

    #[test]
    fn test_20260425_new_empty() {
        let set = CapabilitySet::rvs_new();
        assert!(set.rvs_is_empty());
        assert_eq!(set.rvs_len(), 0);
    }

    #[test]
    fn test_20260709_capability_set_parse_table() {
        let valid_cases = [
            ("valid", "ABIM", "ABIM"),
            ("empty", "", ""),
            ("allow_unknown", "ABEPZ", "ABP"),
            ("validated", "ABSU", "ABSU"),
        ];
        for (name, input, expected) in valid_cases {
            let set = if name == "allow_unknown" {
                CapabilitySet::rvs_from_str_allow_unknown(input)
            } else if name == "validated" {
                CapabilitySet::rvs_from_validated(input)
            } else {
                CapabilitySet::rvs_from_str(input).unwrap()
            };
            assert_eq!(rvs_caps_letters(&set), expected, "{name}");
        }

        let error_cases = [("invalid", "AX", 'X'), ("duplicate", "AAAB", 'A')];
        let mut output = String::new();
        for (name, input, expected_letter) in error_cases {
            let err = CapabilitySet::rvs_from_str(input).unwrap_err();
            output.push_str(&format!("{name}: {err}\n"));
            match (name, err) {
                ("invalid", CapabilityParseError::InvalidLetter { letter }) => {
                    assert_eq!(letter, expected_letter)
                }
                ("duplicate", CapabilityParseError::DuplicateLetter { letter }) => {
                    assert_eq!(letter, expected_letter)
                }
                (_, err) => panic!("unexpected parse error for {name}: {err}"),
            }
        }
        rvs_snapshot_BIS("test_20260709_capability_set_parse_table", &output);
    }

    #[test]
    fn test_20260709_capability_call_rule_table() {
        let cases = [
            ("superset", "ABIM", "ABI", true, ""),
            ("equal", "ABM", "ABM", true, ""),
            ("missing_t", "AB", "ABT", false, "T"),
            ("empty_callee", "A", "", true, ""),
            ("signature_m_ignored", "B", "BM", true, ""),
            ("signature_a_ignored", "B", "BA", true, ""),
            ("signature_u_ignored", "B", "BU", true, ""),
            ("port_propagates", "B", "BP", false, "P"),
            ("port_hides_effects", "P", "BIPST", true, ""),
            ("port_still_required", "BIST", "BIPST", false, "P"),
            ("amu_excluded_from_missing", "B", "ABSTU", false, "ST"),
        ];
        let mut output = String::new();
        for (name, caller, callee, expected_can_call, expected_missing) in cases {
            let caller = CapabilitySet::rvs_from_validated(caller);
            let callee = CapabilitySet::rvs_from_validated(callee);
            let can_call = CapabilityPolicy::rvs_can_call(&caller, &callee);
            assert_eq!(can_call, expected_can_call, "{name}");
            let missing: String = CapabilityPolicy::rvs_missing_for(&caller, &callee)
                .iter()
                .map(|cap| cap.rvs_as_char())
                .collect();
            output.push_str(&format!("{name}: can_call={can_call} missing={missing}\n"));
            assert_eq!(missing, expected_missing, "{name}");
        }
        rvs_snapshot_BIS("test_20260709_capability_call_rule_table", &output);
    }

    #[test]
    fn test_20260702_capability_policy_signature_caps() {
        let mut facts = CapabilityFacts::default();
        facts.has_async = true;
        facts.has_mut_param = true;
        facts.has_static_mut_ref = true;
        facts.has_thread_local_ref = true;

        let caps = CapabilityPolicy::rvs_signature_caps(facts);
        assert!(caps.rvs_contains(Capability::A));
        assert!(caps.rvs_contains(Capability::M));
        assert!(caps.rvs_contains(Capability::S));
        assert!(caps.rvs_contains(Capability::T));
        assert!(caps.rvs_contains(Capability::U));
        assert!(CapabilityPolicy::rvs_requires_signature_cap(
            facts,
            Capability::A
        ));
        assert!(!CapabilityPolicy::rvs_requires_signature_cap(
            facts,
            Capability::P
        ));

        let direct_port_caps = CapabilityPolicy::rvs_port_method_caps();
        assert_eq!(direct_port_caps.rvs_len(), 1);
        assert!(direct_port_caps.rvs_contains(Capability::P));

        let mut port_facts = facts;
        port_facts.is_port_method = true;
        let port_caps = CapabilityPolicy::rvs_signature_caps(port_facts);
        assert_eq!(port_caps.rvs_letters(), "AMPSTU");
        assert!(CapabilityPolicy::rvs_is_propagated_cap(Capability::P));
        assert!(!CapabilityPolicy::rvs_is_propagated_cap(Capability::A));
        assert!(!CapabilityPolicy::rvs_is_propagated_cap(Capability::M));
        assert!(!CapabilityPolicy::rvs_is_propagated_cap(Capability::U));

        let _ = CapabilityFacts::default().rvs_with_static_refs(true, false, true);

        rvs_register_test_coverage(CapabilityFacts::rvs_from_signature);
    }

    #[test]
    fn test_20260709_capability_set_classification_table() {
        let subset_cases = [
            ("subset_true", "AB", "ABIM", true, true, true),
            ("subset_false", "ABT", "ABM", false, false, false),
            ("empty_subset", "", "ABM", true, true, true),
        ];
        let mut output = String::new();
        for (name, set, allowed, is_subset, is_good, is_ok) in subset_cases {
            let set = CapabilitySet::rvs_from_validated(set);
            let allowed = CapabilitySet::rvs_from_validated(allowed);
            output.push_str(&format!(
                "{name}: subset={} good={} ok={}\n",
                set.rvs_is_subset_of(&allowed),
                CapabilityPolicy::rvs_is_good(&set),
                CapabilityPolicy::rvs_is_ok(&set),
            ));
            assert_eq!(set.rvs_is_subset_of(&allowed), is_subset, "{name}");
            assert_eq!(CapabilityPolicy::rvs_is_good(&set), is_good, "{name}");
            assert_eq!(CapabilityPolicy::rvs_is_ok(&set), is_ok, "{name}");
        }

        let good = CapabilityPolicy::rvs_good_caps();
        let ok = CapabilityPolicy::rvs_ok_caps();
        assert_eq!(rvs_caps_letters(&good), "ABM");
        assert_eq!(rvs_caps_letters(&ok), "ABMP");
        output.push_str(&format!(
            "policy: good={} ok={}\n",
            rvs_caps_letters(&good),
            rvs_caps_letters(&ok),
        ));
        rvs_snapshot_BIS("test_20260709_capability_set_classification_table", &output);
    }

    #[test]
    fn test_20260425_is_empty_and_len() {
        let mut set = CapabilitySet::rvs_new();
        assert!(set.rvs_is_empty());
        assert_eq!(set.rvs_len(), 0);
        set.rvs_insert_M(Capability::A);
        assert!(!set.rvs_is_empty());
        assert_eq!(set.rvs_len(), 1);
    }

    #[test]
    fn test_20260425_contains() {
        let set = CapabilitySet::rvs_from_validated("MS");
        assert!(set.rvs_contains(Capability::M));
        assert!(set.rvs_contains(Capability::S));
        assert!(!set.rvs_contains(Capability::A));
    }

    #[test]
    fn test_20260425_iter() {
        let set = CapabilitySet::rvs_from_validated("BAM");
        let caps: Vec<Capability> = set.rvs_iter().collect();
        assert_eq!(caps, vec![Capability::A, Capability::B, Capability::M]);
    }

    #[test]
    fn test_20260425_insert_M() {
        let mut set = CapabilitySet::rvs_new();
        set.rvs_insert_M(Capability::S);
        assert!(set.rvs_contains(Capability::S));
        assert_eq!(set.rvs_len(), 1);
        set.rvs_insert_M(Capability::S);
        assert_eq!(set.rvs_len(), 1);
    }

    #[test]
    fn test_20260709_parse_function_table() {
        let valid_cases = [
            ("suffix", "rvs_write_db_ABI", "write_db", "ABI"),
            ("no_suffix", "rvs_add", "add", ""),
            ("bare_rvs", "rvs_", "", ""),
            ("qualified", "CapsMap::rvs_parse", "parse", ""),
            (
                "qualified_caps",
                "MyMod::rvs_do_thing_ABIM",
                "do_thing",
                "ABIM",
            ),
            (
                "trait_impl",
                "demo::Adapter::rvs_fetch_BI@demo::ApiClient",
                "fetch",
                "BI",
            ),
            (
                "unknown_mixed",
                "rvs_execute_effects_BEIMS",
                "execute_effects",
                "BIMS",
            ),
            ("unknown_only", "rvs_render_art_E", "render_art", ""),
            ("unknown_aeis", "rvs_render_msg_AEIS", "render_msg", "AIS"),
        ];
        let mut output = String::new();
        for (name, input, expected_base, expected_caps) in valid_cases {
            let (base, caps) = rvs_parse_function(input).unwrap();
            output.push_str(&format!("{name}: base={base} caps={caps}\n"));
            assert_eq!(base, expected_base, "{name}");
            assert_eq!(rvs_caps_letters(&caps), expected_caps, "{name}");
        }
        for input in ["foo_bar", "", "rvs_dep::module::plain_BI"] {
            assert_eq!(rvs_parse_function(input), None, "{input}");
        }
        rvs_snapshot_BIS("test_20260709_parse_function_table", &output);
    }

    #[test]
    fn test_20260709_split_and_suffix_table() {
        let trait_impl = "demo::Adapter::rvs_fetch_BI@demo::ApiClient";
        let parsed = ParsedFunctionName::rvs_parse(trait_impl);
        assert_eq!(parsed.segment, "rvs_fetch_BI");
        assert_eq!(parsed.rvs_base_name(), "fetch");
        assert_eq!(parsed.rvs_raw_suffix(), Some("BI"));
        assert!(!ParsedFunctionName::rvs_parse("demo::plain_BI").rvs_has_rvs_prefix());

        let invalid_suffix = ParsedFunctionName::rvs_parse("rvs_write_db_ABI1");
        assert_eq!(invalid_suffix.rvs_base_name(), "write_db_ABI1");
        assert!(invalid_suffix.rvs_known_caps().rvs_is_empty());

        let raw_cases = [
            ("rvs_write_db_ABI", Some("ABI")),
            ("rvs_add", None),
            ("foo_bar", None),
            ("rvs_foo_MBA", Some("MBA")),
            ("rvs_foo_BEIMS", Some("BEIMS")),
            ("rvs_bar_E", Some("E")),
            ("rvs_baz_AEIS", Some("AEIS")),
            (
                "rvs_dep::module::rvs_fetch_BI@rvs_dep::ApiClient",
                Some("BI"),
            ),
            ("rvs_dep::module::plain_BI", None),
        ];
        let mut output = format!(
            "trait_segment={} trait_base={} trait_suffix={:?}\ninvalid_base={}\n",
            parsed.segment,
            parsed.rvs_base_name(),
            parsed.rvs_raw_suffix(),
            invalid_suffix.rvs_base_name(),
        );
        for (input, expected) in raw_cases {
            let actual = ParsedFunctionName::rvs_parse(input).rvs_raw_suffix();
            output.push_str(&format!("{input}: {actual:?}\n"));
            assert_eq!(actual, expected, "{input}");
        }
        rvs_snapshot_BIS("test_20260709_split_and_suffix_table", &output);
    }

    #[test]
    fn test_20260710_parsed_function_name_edge_cases() {
        let cases = [
            ("no_prefix", "demo::plain_BI"),
            ("no_suffix", "rvs_add"),
            ("bare_rvs", "rvs_"),
            ("unknown_only", "rvs_render_E"),
            ("mixed_unknown", "rvs_send_AEIS"),
            ("duplicate", "rvs_copy_AABI"),
            ("non_canonical", "rvs_copy_MBA"),
            ("trait_impl", "demo::Adapter::rvs_fetch_BI@demo::ApiClient"),
            ("invalid_suffix", "rvs_write_ABi"),
        ];
        let mut output = String::new();
        for (label, input) in cases {
            let parsed = ParsedFunctionName::rvs_parse(input);
            let known = rvs_caps_letters(parsed.rvs_known_caps());
            let unknown: String = parsed.rvs_unknown_suffix_letters().iter().collect();
            let duplicates: String = parsed.rvs_duplicate_suffix_letters().iter().collect();
            let declared = parsed
                .rvs_declared_caps()
                .as_ref()
                .map(rvs_caps_letters)
                .unwrap_or_else(|| "none".to_string());
            output.push_str(&format!(
                "{label}: segment={} base={} prefix={} raw={} known={known} unknown={unknown} duplicates={duplicates} canonical={} sorted={} declared={declared}\n",
                parsed.segment,
                parsed.rvs_base_name(),
                parsed.rvs_has_rvs_prefix(),
                parsed.rvs_raw_suffix().unwrap_or("none"),
                parsed.rvs_suffix_is_canonical(),
                parsed.rvs_canonical_suffix().as_deref().unwrap_or("none"),
            ));
        }
        rvs_snapshot_BIS("test_20260710_parsed_function_name_edge_cases", &output);

        assert_eq!(
            output,
            include_str!("../test_out/test_20260710_parsed_function_name_edge_cases.out")
        );
    }

    #[test]
    fn test_20260709_capability_set_display_table() {
        let cases = [("BAM", "{A, B, M}"), ("", "{}")];
        let mut output = String::new();
        for (input, expected) in cases {
            let set = CapabilitySet::rvs_from_validated(input);
            let actual = format!("{set}");
            output.push_str(&format!("{input:?}: {actual}\n"));
            assert_eq!(actual, expected, "{input}");
        }
        rvs_snapshot_BIS("test_20260709_capability_set_display_table", &output);
    }

    #[test]
    fn test_20260725_capability_policy_centralizes_static_and_trait_vote_rules() {
        let mut facts = CapabilityFacts {
            has_async: true,
            ..CapabilityFacts::default()
        };
        facts.rvs_merge_M(CapabilityFacts {
            has_mut_param: true,
            has_static_mut_ref: true,
            has_thread_local_ref: true,
            ..CapabilityFacts::default()
        });
        let static_caps = CapabilityPolicy::rvs_static_caps(facts);
        let implementations = [
            CapabilitySet::rvs_from_validated("BS"),
            CapabilitySet::rvs_from_validated("B"),
            CapabilitySet::rvs_from_validated("IS"),
        ];
        let (selected, threshold, votes) =
            CapabilityPolicy::rvs_at_least_half_vote(implementations.iter())
                .expect("never: non-empty implementations produce a vote");
        let vote_text = votes
            .iter()
            .map(|(capability, count)| format!("{}:{count}", capability.rvs_as_char()))
            .collect::<Vec<_>>()
            .join(",");
        let output = format!(
            "merged_facts={}\nstatic_caps={}\nselected={}\nthreshold={threshold}\nvotes={vote_text}\n",
            CapabilityPolicy::rvs_signature_caps(facts).rvs_letters(),
            static_caps.rvs_letters(),
            selected.rvs_letters(),
        );
        rvs_snapshot_BIS(
            "test_20260725_capability_policy_centralizes_static_and_trait_vote_rules",
            &output,
        );

        assert_eq!(selected.rvs_letters(), "BS");
        assert_eq!(threshold, 2);
    }

    #[test]
    fn test_20260729_persisted_capability_objects_reject_unknown_fields() {
        let facts =
            serde_json::from_str::<CapabilityFacts>(r#"{"has_async":false,"future_fact":true}"#);
        let basis = serde_json::from_str::<CapabilityBasis>(
            r#"{"kind":"trait_vote","implementations":1,"threshold":1,"votes":{},"future_vote":true}"#,
        );
        let output = format!(
            "facts_rejected={}\nbasis_rejected={}\n",
            facts.is_err(),
            basis.is_err(),
        );
        rvs_snapshot_BIS(
            "test_20260729_persisted_capability_objects_reject_unknown_fields",
            &output,
        );

        assert!(facts.is_err());
        assert!(basis.is_err());
    }
}
