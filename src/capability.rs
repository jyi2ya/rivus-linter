use std::collections::BTreeSet;
use std::fmt;

/// 能力之七德：异步、阻塞、读写、可变、副作用、线程、不安。
/// 七德既立，函数之名即为契约，调用之际便有章法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    A,
    B,
    I,
    M,
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

const VALID_SUFFIX_CHARS: &[char] = &['A', 'B', 'I', 'M', 'S', 'T', 'U'];

/// 一组能力，如同一面旗——旗上画的，便是这函数的本事。
/// 旗上没画的，便是它干不了的。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    /// 构造一个空的能力集。
    pub fn rvs_new() -> Self {
        Self(BTreeSet::new())
    }

    /// 从后缀字符串解析能力集。遇到非法字母返回错误。
    pub fn rvs_from_str(s: &str) -> Result<Self, CapabilityParseError> {
        let mut set = BTreeSet::new();
        for c in s.chars() {
            let cap = Capability::rvs_from_char(c).ok_or(CapabilityParseError::InvalidLetter(c))?;
            set.insert(cap);
        }
        Ok(Self(set))
    }

    /// 从已经校验过的后缀字符串解析能力集（预期任何字母都合法）。
    pub fn rvs_from_validated(s: &str) -> Self {
        let mut set = BTreeSet::new();
        for c in s.chars() {
            let cap = match c {
                'A' => Capability::A,
                'B' => Capability::B,
                'I' => Capability::I,
                'M' => Capability::M,
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

    /// 调用之规：我有，方可调你。
    /// 你有的每一个能力，我都必须有，方为合规。
    ///
    /// 但 A、M、U 三个签名推断能力不参与调用规则检查——它们只从函数自身的
    /// 签名推断（has_async / has_mut_param / is_unsafe_fn），不通过传播获得。
    /// 强制检查它们会导致 annotate 与 check 不一致（annotate 不传播这些能力，
    /// 但 check 会报违规）。
    pub fn rvs_can_call(&self, other: &Self) -> bool {
        other.0.iter().all(|cap| {
            matches!(cap, Capability::A | Capability::M | Capability::U) || self.0.contains(cap)
        })
    }

    /// 算一算，调它还差几道功夫。
    /// 同样排除 A、M、U（签名推断能力，不参与调用规则）。
    pub fn rvs_missing_for(&self, other: &Self) -> BTreeSet<Capability> {
        other
            .0
            .iter()
            .filter(|cap| !matches!(cap, Capability::A | Capability::M | Capability::U))
            .copied()
            .filter(|cap| !self.0.contains(cap))
            .collect()
    }

    /// 我的能力是否全在你允许的范围之内。
    pub fn rvs_is_subset_of(&self, allowed: &Self) -> bool {
        self.0.iter().all(|cap| allowed.0.contains(cap))
    }

    /// 好函数的及格线：ABM 三德以内，便是善。
    pub fn rvs_from_good_caps() -> Self {
        Self(
            [Capability::A, Capability::B, Capability::M]
                .into_iter()
                .collect(),
        )
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

#[derive(Debug, thiserror::Error)]
pub enum CapabilityParseError {
    #[error("invalid capability letter: '{0}'")]
    InvalidLetter(char),
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
    debug_assert!(!name.is_empty());

    if let Some(result) = rvs_parse_segment(name) {
        return Some(result);
    }
    let last_segment = name.rsplit("::").next()?;
    rvs_parse_segment(last_segment)
}

/// 拆解单个片段：去掉 rvs_ 前缀后，萃取能力后缀。
///
/// 后缀必须全是大写字母。若所有字母都是合法能力字母（ABIMSTU），
/// 直接萃取。若含未知大写字母（如 E），仍萃取已知部分，
/// 由调用方负责报告未知字母警告。
fn rvs_parse_segment(name: &str) -> Option<(&str, CapabilitySet)> {
    let rest = name.strip_prefix("rvs_")?;

    if let Some(pos) = rest.rfind('_') {
        let potential_suffix = rest.get(pos + 1..).unwrap_or("");
        let base = rest.get(..pos).unwrap_or("");

        if !potential_suffix.is_empty() && potential_suffix.chars().all(|c| c.is_ascii_uppercase())
        {
            let caps = CapabilitySet::rvs_from_str_allow_unknown(potential_suffix);
            return Some((base, caps));
        }
    }

    Some((rest, CapabilitySet::rvs_new()))
}

/// 从 rvs_ 函数名中萃取原始后缀字符串（未排序、未去重）。
/// 用于检查命名规范（C4 字母序、C5 重复字母、未知字母）。
/// 后缀必须全是大写字母才视为有效。
pub fn rvs_extract_raw_suffix(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("rvs_")
        && let Some(pos) = rest.rfind('_')
    {
        let potential_suffix = rest.get(pos + 1..).unwrap_or("");
        if !potential_suffix.is_empty() && potential_suffix.chars().all(|c| c.is_ascii_uppercase())
        {
            return potential_suffix.to_string();
        }
    }
    String::new()
}

/// 从原始后缀中萃取未知（非 ABIMSTU）的大写字母，按出现顺序去重。
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
            CapabilityParseError::InvalidLetter(c) => assert_eq!(c, 'X'),
        }
    }

    #[test]
    fn test_20260425_from_str_empty() {
        let set = CapabilitySet::rvs_from_str("").unwrap();
        assert!(set.rvs_is_empty());
    }

    #[test]
    fn test_20260425_from_str_dedup() {
        let set = CapabilitySet::rvs_from_str("AAAB").unwrap();
        assert_eq!(set.rvs_len(), 2);
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
        assert!(caller.rvs_can_call(&callee));
    }

    #[test]
    fn test_20260425_can_call_equal() {
        let a = CapabilitySet::rvs_from_validated("ABM");
        let b = CapabilitySet::rvs_from_validated("ABM");
        assert!(a.rvs_can_call(&b));
    }

    #[test]
    fn test_20260425_can_call_missing_cap() {
        let caller = CapabilitySet::rvs_from_validated("AB");
        let callee = CapabilitySet::rvs_from_validated("ABT");
        assert!(!caller.rvs_can_call(&callee));
    }

    #[test]
    fn test_20260425_can_call_empty_callee() {
        let caller = CapabilitySet::rvs_from_validated("A");
        let callee = CapabilitySet::rvs_new();
        assert!(caller.rvs_can_call(&callee));
    }

    #[test]
    fn test_20260425_missing_for_no_missing() {
        let a = CapabilitySet::rvs_from_validated("ABIM");
        let b = CapabilitySet::rvs_from_validated("AB");
        assert!(a.rvs_missing_for(&b).is_empty());
    }

    #[test]
    fn test_20260425_missing_for_has_missing() {
        let a = CapabilitySet::rvs_from_validated("AB");
        let b = CapabilitySet::rvs_from_validated("ABT");
        let missing = a.rvs_missing_for(&b);
        assert_eq!(missing.len(), 1);
        assert!(missing.contains(&Capability::T));
    }

    #[test]
    fn test_20260614_can_call_excludes_amu() {
        // A, M, U are signature-only capabilities — they don't participate
        // in the call rule. A function without M can call one with M, etc.
        let caller = CapabilitySet::rvs_from_validated("B");
        let callee_m = CapabilitySet::rvs_from_validated("BM");
        let callee_a = CapabilitySet::rvs_from_validated("BA");
        let callee_u = CapabilitySet::rvs_from_validated("BU");
        assert!(caller.rvs_can_call(&callee_m), "missing M should not block");
        assert!(caller.rvs_can_call(&callee_a), "missing A should not block");
        assert!(caller.rvs_can_call(&callee_u), "missing U should not block");
    }

    #[test]
    fn test_20260614_missing_for_excludes_amu() {
        let caller = CapabilitySet::rvs_from_validated("B");
        let callee = CapabilitySet::rvs_from_validated("ABSTU");
        let missing = caller.rvs_missing_for(&callee);
        // Only S and T should be missing — A, M, U are excluded from call rule
        assert_eq!(missing.len(), 2);
        assert!(missing.contains(&Capability::T));
        assert!(missing.contains(&Capability::S));
    }

    #[test]
    fn test_20260425_is_subset_of_true() {
        let set = CapabilitySet::rvs_from_validated("AB");
        let allowed = CapabilitySet::rvs_from_validated("ABIM");
        assert!(set.rvs_is_subset_of(&allowed));
    }

    #[test]
    fn test_20260425_is_subset_of_false() {
        let set = CapabilitySet::rvs_from_validated("ABT");
        let allowed = CapabilitySet::rvs_from_validated("ABM");
        assert!(!set.rvs_is_subset_of(&allowed));
    }

    #[test]
    fn test_20260425_is_subset_of_empty() {
        let empty = CapabilitySet::rvs_new();
        let allowed = CapabilitySet::rvs_from_validated("ABM");
        assert!(empty.rvs_is_subset_of(&allowed));
    }

    #[test]
    fn test_20260425_from_good_caps() {
        let good = CapabilitySet::rvs_from_good_caps();
        assert!(good.rvs_contains(Capability::A));
        assert!(good.rvs_contains(Capability::B));
        assert!(good.rvs_contains(Capability::M));
        assert!(!good.rvs_contains(Capability::I));
        assert!(!good.rvs_contains(Capability::S));
        assert!(!good.rvs_contains(Capability::T));
        assert!(!good.rvs_contains(Capability::U));
        assert_eq!(good.rvs_len(), 3);
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
