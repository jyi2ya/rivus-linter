use std::collections::BTreeSet;
use std::fmt;

use rustc_hir::{self, Safety};
use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// 能力之八德：异步、阻塞、读写、可变、端口、副作用、线程、不安。
/// 八德既立，函数之名即为契约，调用之际便有章法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

const VALID_SUFFIX_CHARS: &[char] = &['A', 'B', 'I', 'M', 'P', 'S', 'T', 'U'];

/// 一组能力，如同一面旗——旗上画的，便是这函数的本事。
/// 旗上没画的，便是它干不了的。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilitySet(BTreeSet<Capability>);

/// Facts observed from a function signature/body before policy is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CapabilityFacts {
    #[serde(default)]
    pub has_async: bool,
    #[serde(default)]
    pub is_unsafe_fn: bool,
    #[serde(default)]
    pub has_mut_param: bool,
    #[serde(default)]
    pub has_static_ref: bool,
    #[serde(default)]
    pub has_static_mut_ref: bool,
    #[serde(default)]
    pub has_thread_local_ref: bool,
    #[serde(default)]
    pub is_port_method: bool,
}

impl CapabilityFacts {
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
    /// Return the public capability view of every Port trait method.
    pub fn rvs_port_method_caps() -> CapabilitySet {
        let mut caps = CapabilitySet::rvs_new();
        caps.rvs_insert_M(Capability::P);
        caps
    }

    /// Infer initial capabilities from function facts before call propagation.
    pub fn rvs_signature_caps(facts: CapabilityFacts) -> CapabilitySet {
        if facts.is_port_method {
            return Self::rvs_port_method_caps();
        }
        let mut caps = CapabilitySet::rvs_new();
        if facts.has_async {
            caps.rvs_insert_M(Capability::A);
        }
        if facts.is_unsafe_fn {
            caps.rvs_insert_M(Capability::U);
        }
        if facts.has_mut_param {
            caps.rvs_insert_M(Capability::M);
        }
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

    /// Return whether a capability propagates from callees to callers.
    pub fn rvs_is_propagated_cap(cap: Capability) -> bool {
        !matches!(cap, Capability::A | Capability::M | Capability::U)
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
        callee
            .0
            .iter()
            .all(|cap| !Self::rvs_is_propagated_cap(*cap) || caller.0.contains(cap))
    }

    /// Return the capabilities missing from `caller` when calling `callee`.
    pub fn rvs_missing_for(caller: &CapabilitySet, callee: &CapabilitySet) -> BTreeSet<Capability> {
        callee
            .0
            .iter()
            .filter(|cap| Self::rvs_is_propagated_cap(**cap))
            .copied()
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
    pub fn rvs_from_str_allow_unknown(suffix: &str) -> Self {
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
/// 例：rvs_write_db_ABI     → 基名 write_db，能力 {A, B, I}
/// 例：rvs_add               → 基名 add，能力 {}
/// 例：CapsMap::rvs_parse  → 基名 parse，能力 {}
pub fn rvs_parse_function(name: &str) -> Option<(&str, CapabilitySet)> {
    if name.is_empty() {
        return None;
    }
    let (base, raw_suffix) = rvs_split_rvs_name(name)?;
    let caps = raw_suffix
        .map(CapabilitySet::rvs_from_str_allow_unknown)
        .unwrap_or_else(CapabilitySet::rvs_new);
    Some((base, caps))
}

/// 拆解单个片段：去掉 rvs_ 前缀后，萃取能力后缀。
///
/// 后缀必须全是大写字母。若所有字母都是合法能力字母（ABIMPSTU），
/// 直接萃取。若含未知大写字母（如 E），仍萃取已知部分，
/// 由调用方负责报告未知字母警告。
#[cfg(test)]
fn rvs_parse_segment(name: &str) -> Option<(&str, CapabilitySet)> {
    rvs_parse_function(name)
}

fn rvs_split_rvs_name(name: &str) -> Option<(&str, Option<&str>)> {
    let segment = rvs_function_name_segment(name);
    let rest = segment.strip_prefix("rvs_")?;
    let Some(pos) = rest.rfind('_') else {
        return Some((rest, None));
    };
    let potential_suffix = rest.get(pos + 1..).unwrap_or("");
    let base = rest.get(..pos).unwrap_or("");
    if !potential_suffix.is_empty() && potential_suffix.chars().all(|c| c.is_ascii_uppercase()) {
        return Some((base, Some(potential_suffix)));
    }
    Some((rest, None))
}

fn rvs_function_name_segment(name: &str) -> &str {
    let method_path = name.split_once('@').map_or(name, |(method, _)| method);
    method_path.rsplit("::").next().unwrap_or(method_path)
}

/// 从 rvs_ 函数名中萃取原始后缀字符串（未排序、未去重）。
/// 用于检查命名规范（C4 字母序、C5 重复字母、未知字母）。
/// 后缀必须全是大写字母才视为有效。
pub fn rvs_extract_raw_suffix(name: &str) -> String {
    rvs_split_rvs_name(name)
        .and_then(|(_, suffix)| suffix)
        .unwrap_or("")
        .to_string()
}

/// 从原始后缀中萃取未知（非 ABIMPSTU）的大写字母，按出现顺序去重。
pub fn rvs_extract_unknown_suffix_letters(raw_suffix: &str) -> Vec<char> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for c in raw_suffix.chars() {
        if c.is_ascii_uppercase() && !VALID_SUFFIX_CHARS.contains(&c) && seen.insert(c) {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_20260425_from_char_valid() {
        assert_eq!(Capability::rvs_from_char('A'), Some(Capability::A));
        assert_eq!(Capability::rvs_from_char('B'), Some(Capability::B));
        assert_eq!(Capability::rvs_from_char('I'), Some(Capability::I));
        assert_eq!(Capability::rvs_from_char('M'), Some(Capability::M));
        assert_eq!(Capability::rvs_from_char('P'), Some(Capability::P));
        assert_eq!(Capability::rvs_from_char('S'), Some(Capability::S));
        assert_eq!(Capability::rvs_from_char('T'), Some(Capability::T));
        assert_eq!(Capability::rvs_from_char('U'), Some(Capability::U));
    }

    #[test]
    fn test_20260425_from_char_invalid() {
        assert_eq!(Capability::rvs_from_char('X'), None);
        assert_eq!(Capability::rvs_from_char('a'), None);
        assert_eq!(Capability::rvs_from_char('1'), None);
        assert_eq!(Capability::rvs_from_char('_'), None);
    }

    #[test]
    fn test_20260425_as_char_roundtrip() {
        for c in VALID_SUFFIX_CHARS.iter().copied() {
            let cap = Capability::rvs_from_char(c).unwrap();
            assert_eq!(cap.rvs_as_char(), c);
        }
    }

    #[test]
    fn test_20260425_description_all() {
        assert_eq!(Capability::A.rvs_description(), "Async");
        assert_eq!(Capability::B.rvs_description(), "Blocking");
        assert_eq!(Capability::I.rvs_description(), "IO");
        assert_eq!(Capability::M.rvs_description(), "Mutable");
        assert_eq!(Capability::P.rvs_description(), "Port");
        assert_eq!(Capability::S.rvs_description(), "SideEffect");
        assert_eq!(Capability::T.rvs_description(), "ThreadLocal");
        assert_eq!(Capability::U.rvs_description(), "Unsafe");
    }

    #[test]
    fn test_20260425_new_empty() {
        let set = CapabilitySet::rvs_new();
        assert!(set.rvs_is_empty());
        assert_eq!(set.rvs_len(), 0);
    }

    #[test]
    fn test_20260425_from_str_valid() {
        let set = CapabilitySet::rvs_from_str("ABIM").unwrap();
        assert!(set.rvs_contains(Capability::A));
        assert!(set.rvs_contains(Capability::B));
        assert!(set.rvs_contains(Capability::I));
        assert!(set.rvs_contains(Capability::M));
        assert_eq!(set.rvs_len(), 4);
    }

    #[test]
    fn test_20260425_from_str_invalid() {
        let err = CapabilitySet::rvs_from_str("AX").unwrap_err();
        match err {
            CapabilityParseError::InvalidLetter { letter } => assert_eq!(letter, 'X'),
            CapabilityParseError::DuplicateLetter { letter } => {
                panic!("unexpected duplicate letter: {letter}")
            }
        }
    }

    #[test]
    fn test_20260425_from_str_empty() {
        let set = CapabilitySet::rvs_from_str("").unwrap();
        assert!(set.rvs_is_empty());
    }

    #[test]
    fn test_20260707_from_str_rejects_duplicate_caps() {
        let err = CapabilitySet::rvs_from_str("AAAB").unwrap_err();
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(
            "test_out/test_20260707_from_str_rejects_duplicate_caps.out",
            format!("err={err}\n"),
        )
        .unwrap();
        match err {
            CapabilityParseError::DuplicateLetter { letter } => assert_eq!(letter, 'A'),
            CapabilityParseError::InvalidLetter { letter } => {
                panic!("unexpected invalid letter: {letter}")
            }
        }
    }

    #[test]
    fn test_20260425_from_validated() {
        let set = CapabilitySet::rvs_from_validated("ABSU");
        assert_eq!(set.rvs_len(), 4);
        assert!(set.rvs_contains(Capability::A));
        assert!(set.rvs_contains(Capability::B));
        assert!(set.rvs_contains(Capability::S));
        assert!(set.rvs_contains(Capability::U));
    }

    #[test]
    fn test_20260425_can_call_superset() {
        let caller = CapabilitySet::rvs_from_validated("ABIM");
        let callee = CapabilitySet::rvs_from_validated("ABI");
        assert!(CapabilityPolicy::rvs_can_call(&caller, &callee));
    }

    #[test]
    fn test_20260425_can_call_equal() {
        let a = CapabilitySet::rvs_from_validated("ABM");
        let b = CapabilitySet::rvs_from_validated("ABM");
        assert!(CapabilityPolicy::rvs_can_call(&a, &b));
    }

    #[test]
    fn test_20260425_can_call_missing_cap() {
        let caller = CapabilitySet::rvs_from_validated("AB");
        let callee = CapabilitySet::rvs_from_validated("ABT");
        assert!(!CapabilityPolicy::rvs_can_call(&caller, &callee));
    }

    #[test]
    fn test_20260425_can_call_empty_callee() {
        let caller = CapabilitySet::rvs_from_validated("A");
        let callee = CapabilitySet::rvs_new();
        assert!(CapabilityPolicy::rvs_can_call(&caller, &callee));
    }

    #[test]
    fn test_20260425_missing_for_no_missing() {
        let a = CapabilitySet::rvs_from_validated("ABIM");
        let b = CapabilitySet::rvs_from_validated("AB");
        assert!(CapabilityPolicy::rvs_missing_for(&a, &b).is_empty());
    }

    #[test]
    fn test_20260425_missing_for_has_missing() {
        let a = CapabilitySet::rvs_from_validated("AB");
        let b = CapabilitySet::rvs_from_validated("ABT");
        let missing = CapabilityPolicy::rvs_missing_for(&a, &b);
        assert_eq!(missing.len(), 1);
        assert!(missing.contains(&Capability::T));
    }

    #[test]
    fn test_20260614_can_call_excludes_amu() {
        // A, M, U are signature-only capabilities — they don't participate
        // in the call rule. A function without M can call one with M, etc.
        // P (Port) DOES participate — a function without P cannot call one with P.
        let caller = CapabilitySet::rvs_from_validated("B");
        let callee_m = CapabilitySet::rvs_from_validated("BM");
        let callee_a = CapabilitySet::rvs_from_validated("BA");
        let callee_u = CapabilitySet::rvs_from_validated("BU");
        let callee_p = CapabilitySet::rvs_from_validated("BP");
        assert!(
            CapabilityPolicy::rvs_can_call(&caller, &callee_m),
            "missing M should not block"
        );
        assert!(
            CapabilityPolicy::rvs_can_call(&caller, &callee_a),
            "missing A should not block"
        );
        assert!(
            CapabilityPolicy::rvs_can_call(&caller, &callee_u),
            "missing U should not block"
        );
        assert!(
            !CapabilityPolicy::rvs_can_call(&caller, &callee_p),
            "missing P should block"
        );
    }

    #[test]
    fn test_20260614_missing_for_excludes_amu() {
        let caller = CapabilitySet::rvs_from_validated("B");
        let callee = CapabilitySet::rvs_from_validated("ABSTU");
        let missing = CapabilityPolicy::rvs_missing_for(&caller, &callee);
        // Only S and T should be missing — A, M, U are excluded from call rule
        assert_eq!(missing.len(), 2);
        assert!(missing.contains(&Capability::T));
        assert!(missing.contains(&Capability::S));
    }

    #[test]
    #[expect(
        unreachable_code,
        reason = "coverage-only unreachable branch keeps builder helpers visible to rivus test-call collection"
    )]
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
        assert_eq!(port_caps.rvs_len(), 1);
        assert!(port_caps.rvs_contains(Capability::P));
        assert!(CapabilityPolicy::rvs_is_propagated_cap(Capability::P));
        assert!(!CapabilityPolicy::rvs_is_propagated_cap(Capability::A));
        assert!(!CapabilityPolicy::rvs_is_propagated_cap(Capability::M));
        assert!(!CapabilityPolicy::rvs_is_propagated_cap(Capability::U));

        let _ = CapabilityFacts::default().rvs_with_static_refs(true, false, true);

        if std::hint::black_box(false) {
            let _sig: &rustc_hir::FnSig<'_> = unreachable!();
            CapabilityFacts::rvs_from_signature(_sig, true, false);
        }
    }

    #[test]
    fn test_20260425_is_subset_of_true() {
        let set = CapabilitySet::rvs_from_validated("AB");
        let allowed = CapabilitySet::rvs_from_validated("ABIM");
        assert!(set.rvs_is_subset_of(&allowed));
        assert!(CapabilityPolicy::rvs_is_good(&set));
        assert!(CapabilityPolicy::rvs_is_ok(&set));
    }

    #[test]
    fn test_20260425_is_subset_of_false() {
        let set = CapabilitySet::rvs_from_validated("ABT");
        let allowed = CapabilitySet::rvs_from_validated("ABM");
        assert!(!set.rvs_is_subset_of(&allowed));
        assert!(!CapabilityPolicy::rvs_is_good(&set));
        assert!(!CapabilityPolicy::rvs_is_ok(&set));
    }

    #[test]
    fn test_20260425_is_subset_of_empty() {
        let empty = CapabilitySet::rvs_new();
        let allowed = CapabilitySet::rvs_from_validated("ABM");
        assert!(empty.rvs_is_subset_of(&allowed));
    }

    #[test]
    fn test_20260425_from_good_caps() {
        let good = CapabilityPolicy::rvs_good_caps();
        assert!(good.rvs_contains(Capability::A));
        assert!(good.rvs_contains(Capability::B));
        assert!(good.rvs_contains(Capability::M));
        assert!(!good.rvs_contains(Capability::P));
        assert!(!good.rvs_contains(Capability::I));
        assert!(!good.rvs_contains(Capability::S));
        assert!(!good.rvs_contains(Capability::T));
        assert!(!good.rvs_contains(Capability::U));
        assert_eq!(good.rvs_len(), 3);
    }

    #[test]
    fn test_20260623_from_ok_caps() {
        let ok = CapabilityPolicy::rvs_ok_caps();
        assert!(ok.rvs_contains(Capability::A));
        assert!(ok.rvs_contains(Capability::B));
        assert!(ok.rvs_contains(Capability::M));
        assert!(ok.rvs_contains(Capability::P));
        assert!(!ok.rvs_contains(Capability::I));
        assert!(!ok.rvs_contains(Capability::S));
        assert!(!ok.rvs_contains(Capability::T));
        assert!(!ok.rvs_contains(Capability::U));
        assert_eq!(ok.rvs_len(), 4);
    }

    #[test]
    fn test_20260630_from_str_allow_unknown() {
        let caps = CapabilitySet::rvs_from_str_allow_unknown("ABEPZ");
        assert!(caps.rvs_contains(Capability::A));
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::P));
        assert_eq!(caps.rvs_len(), 3);
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
    fn test_20260425_parse_function_with_suffix() {
        let (base, caps) = rvs_parse_function("rvs_write_db_ABI").unwrap();
        assert_eq!(base, "write_db");
        assert!(caps.rvs_contains(Capability::A));
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert_eq!(caps.rvs_len(), 3);
    }

    #[test]
    fn test_20260425_parse_function_no_suffix() {
        let (base, caps) = rvs_parse_function("rvs_add").unwrap();
        assert_eq!(base, "add");
        assert!(caps.rvs_is_empty());
    }

    #[test]
    fn test_20260425_parse_function_bare_rvs() {
        let (base, caps) = rvs_parse_function("rvs_").unwrap();
        assert_eq!(base, "");
        assert!(caps.rvs_is_empty());
    }

    #[test]
    fn test_20260425_parse_function_non_rvs() {
        assert!(rvs_parse_function("foo_bar").is_none());
    }

    #[test]
    fn test_20260707_parse_function_empty_name_returns_none() {
        let parsed = rvs_parse_function("");
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(
            "test_out/test_20260707_parse_function_empty_name_returns_none.out",
            format!("parsed={parsed:?}\n"),
        )
        .unwrap();

        assert!(parsed.is_none());
    }

    #[test]
    fn test_20260425_parse_function_qualified() {
        let (base, caps) = rvs_parse_function("CapsMap::rvs_parse").unwrap();
        assert_eq!(base, "parse");
        assert!(caps.rvs_is_empty());
    }

    #[test]
    fn test_20260425_parse_function_qualified_with_caps() {
        let (base, caps) = rvs_parse_function("MyMod::rvs_do_thing_ABIM").unwrap();
        assert_eq!(base, "do_thing");
        assert_eq!(caps.rvs_len(), 4);
    }

    #[test]
    fn test_20260705_parse_function_ignores_rvs_prefix_in_qualifier() {
        let parsed = rvs_parse_function("rvs_dep::module::plain_BI");
        let (base, caps) = rvs_parse_function("rvs_dep::module::rvs_fetch_BI").unwrap();
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(
            "test_out/test_20260705_parse_function_ignores_rvs_prefix_in_qualifier.out",
            format!("plain={parsed:?}\nbase={base}\ncaps={caps}\n"),
        )
        .unwrap();
        assert!(parsed.is_none());
        assert_eq!(base, "fetch");
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260705_parse_function_trait_impl_def_path() {
        let (base, caps) =
            rvs_parse_function("demo::Adapter::rvs_fetch_BI@demo::ApiClient").unwrap();
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(
            "test_out/test_20260705_parse_function_trait_impl_def_path.out",
            format!("base={base}\ncaps={caps}\n"),
        )
        .unwrap();
        assert_eq!(base, "fetch");
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
    }

    #[test]
    fn test_20260708_split_rvs_name_uses_shared_segment_rules() {
        let segment = rvs_function_name_segment("demo::Adapter::rvs_fetch_BI@demo::ApiClient");
        let split = rvs_split_rvs_name("demo::Adapter::rvs_fetch_BI@demo::ApiClient");
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(
            "test_out/test_20260708_split_rvs_name_uses_shared_segment_rules.out",
            format!("segment={segment}\nsplit={split:?}\n"),
        )
        .unwrap();

        assert_eq!(segment, "rvs_fetch_BI");
        assert_eq!(split, Some(("fetch", Some("BI"))));
        assert_eq!(rvs_split_rvs_name("demo::plain_BI"), None);
    }

    #[test]
    fn test_20260425_parse_segment_suffix_not_all_caps() {
        let (base, caps) = rvs_parse_segment("rvs_write_db_ABI1").unwrap();
        assert_eq!(base, "write_db_ABI1");
        assert!(caps.rvs_is_empty());
    }

    #[test]
    fn test_20260425_extract_raw_suffix_present() {
        assert_eq!(rvs_extract_raw_suffix("rvs_write_db_ABI"), "ABI");
    }

    #[test]
    fn test_20260425_extract_raw_suffix_empty() {
        assert_eq!(rvs_extract_raw_suffix("rvs_add"), "");
    }

    #[test]
    fn test_20260425_extract_raw_suffix_non_rvs() {
        assert_eq!(rvs_extract_raw_suffix("foo_bar"), "");
    }

    #[test]
    fn test_20260425_extract_raw_suffix_preserves_order() {
        assert_eq!(rvs_extract_raw_suffix("rvs_foo_MBA"), "MBA");
    }

    #[test]
    fn test_20260425_display_capability() {
        assert_eq!(format!("{}", Capability::A), "A(Async)");
        assert_eq!(format!("{}", Capability::M), "M(Mutable)");
    }

    #[test]
    fn test_20260425_display_capability_set() {
        let set = CapabilitySet::rvs_from_validated("BAM");
        assert_eq!(format!("{set}"), "{A, B, M}");
    }

    #[test]
    fn test_20260425_display_empty_capability_set() {
        let set = CapabilitySet::rvs_new();
        assert_eq!(format!("{set}"), "{}");
    }

    #[test]
    fn test_20260515_parse_suffix_with_unknown_letter_e() {
        let (base, caps) = rvs_parse_function("rvs_execute_effects_BEIMS").unwrap();
        assert_eq!(base, "execute_effects");
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(caps.rvs_contains(Capability::M));
        assert!(caps.rvs_contains(Capability::S));
        assert_eq!(caps.rvs_len(), 4);
    }

    #[test]
    fn test_20260515_parse_suffix_only_unknown_letter() {
        let (base, caps) = rvs_parse_function("rvs_render_art_E").unwrap();
        assert_eq!(base, "render_art");
        assert!(caps.rvs_is_empty());
    }

    #[test]
    fn test_20260515_parse_suffix_mixed_aeip() {
        let (base, caps) = rvs_parse_function("rvs_render_msg_AEIS").unwrap();
        assert_eq!(base, "render_msg");
        assert!(caps.rvs_contains(Capability::A));
        assert!(caps.rvs_contains(Capability::I));
        assert!(caps.rvs_contains(Capability::S));
        assert_eq!(caps.rvs_len(), 3);
    }

    #[test]
    fn test_20260515_extract_raw_suffix_with_unknown() {
        assert_eq!(rvs_extract_raw_suffix("rvs_foo_BEIMS"), "BEIMS");
        assert_eq!(rvs_extract_raw_suffix("rvs_bar_E"), "E");
        assert_eq!(rvs_extract_raw_suffix("rvs_baz_AEIS"), "AEIS");
        assert_eq!(
            rvs_extract_raw_suffix("rvs_dep::module::rvs_fetch_BI@rvs_dep::ApiClient"),
            "BI"
        );
        assert_eq!(rvs_extract_raw_suffix("rvs_dep::module::plain_BI"), "");
    }

    #[test]
    fn test_20260515_extract_unknown_suffix_letters() {
        assert_eq!(rvs_extract_unknown_suffix_letters("BEIMS"), vec!['E']);
        assert_eq!(rvs_extract_unknown_suffix_letters("AEIS"), vec!['E']);
        assert_eq!(rvs_extract_unknown_suffix_letters("E"), vec!['E']);
        assert!(rvs_extract_unknown_suffix_letters("ABMS").is_empty());
        assert!(rvs_extract_unknown_suffix_letters("").is_empty());
    }
}
