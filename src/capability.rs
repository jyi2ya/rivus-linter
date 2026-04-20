use std::collections::BTreeSet;
use std::fmt;

/// 能力之七德：异步、阻塞、读写、可变、惊慌、副作用、线程、不安。
/// 七德既立，函数之名即为契约，调用之际便有章法。
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

    pub fn rvs_description(self) -> &'static str {
        match self {
            Self::A => "Async",
            Self::B => "Blocking",
            Self::I => "IO",
            Self::M => "Mutable",
            Self::P => "Panic",
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

#[allow(non_snake_case)]
impl CapabilitySet {
    pub fn rvs_new() -> Self {
        Self(BTreeSet::new())
    }

    pub fn rvs_from_str(s: &str) -> Result<Self, CapabilityParseError> {
        let mut set = BTreeSet::new();
        for c in s.chars() {
            let cap = Capability::rvs_from_char(c)
                .ok_or(CapabilityParseError::InvalidLetter(c))?;
            set.insert(cap);
        }
        Ok(Self(set))
    }

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

    /// 调用之规：我有，方可调你。
    /// 你有的每一个能力，我都必须有，方为合规。
    pub fn rvs_can_call(&self, other: &Self) -> bool {
        other.0.iter().all(|cap| self.0.contains(cap))
    }

    /// 算一算，调它还差几道功夫。
    pub fn rvs_missing_for(&self, other: &Self) -> BTreeSet<Capability> {
        other.0.difference(&self.0).copied().collect()
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

    pub fn rvs_is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn rvs_contains(&self, cap: Capability) -> bool {
        self.0.contains(&cap)
    }

    pub fn rvs_iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    pub fn rvs_len(&self) -> usize {
        self.0.len()
    }

    pub fn insert(&mut self, cap: Capability) {
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
pub fn parse_rvs_function(name: &str) -> Option<(&str, CapabilitySet)> {
    debug_assert!(!name.is_empty());

    if let Some(result) = parse_rvs_segment(name) {
        return Some(result);
    }
    let last_segment = name.rsplit("::").next()?;
    parse_rvs_segment(last_segment)
}

/// 拆解单个片段：去掉 rvs_ 前缀后，萃取能力后缀。
fn parse_rvs_segment(name: &str) -> Option<(&str, CapabilitySet)> {
    let rest = name.strip_prefix("rvs_")?;

    if let Some(pos) = rest.rfind('_') {
        let potential_suffix = &rest[pos + 1..];
        let base = &rest[..pos];

        if !potential_suffix.is_empty()
            && potential_suffix
                .chars()
                .all(|c| VALID_SUFFIX_CHARS.contains(&c))
        {
            let caps = CapabilitySet::rvs_from_validated(potential_suffix);
            return Some((base, caps));
        }
    }

    Some((rest, CapabilitySet::rvs_new()))
}

/// 从 rvs_ 函数名中萃取原始后缀字符串（未排序、未去重）。
/// 用于检查命名规范（C4 字母序、C5 重复字母）。
pub fn rvs_extract_raw_suffix(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("rvs_")
        && let Some(pos) = rest.rfind('_')
    {
        let potential_suffix = &rest[pos + 1..];
        if !potential_suffix.is_empty()
            && potential_suffix
                .chars()
                .all(|c| VALID_SUFFIX_CHARS.contains(&c))
        {
            return potential_suffix.to_string();
        }
    }
    String::new()
}
