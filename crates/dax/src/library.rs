//! The library functions: plain scalar functions of their arguments (no filter context,
//! no tables), declared once here with their argument kinds.
//!
//! Each one has an arm in the SQL compiler (`charted_compiler::visual::lib_sql`), in
//! the reference interpreter (`charted_compiler::interp::lib_fn`) and in the Ossie
//! translator, all exhaustive matches, so adding a row here fails to build until all
//! three handle it. The arguments reach those arms already read as their kind says,
//! the same way on both sides, so blank handling can't drift between them.
//!
//! Descriptions and parameter names for the editor live in `catalog.rs`.

/// How an argument is read before the function sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// A number (TRUE is 1); blank counts as zero.
    Num,
    /// A number (TRUE is 1); a blank argument makes the result blank.
    NumOrBlank,
    /// A whole number: decimals are truncated, blank counts as zero.
    Int,
    /// Text; blank counts as empty text, numbers and dates as they're written.
    Text,
}

/// What a library function returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Returns {
    Num,
    Text,
    Bool,
    Date,
}

macro_rules! library {
    ($( $variant:ident = $name:literal ( $($kind:ident),* $(; $($opt:ident),+)? ) -> $ret:ident; )*) => {
        /// A library function (see the module docs).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum LibFn {
            $($variant),*
        }

        impl LibFn {
            pub const ALL: &'static [LibFn] = &[$(LibFn::$variant),*];

            /// The DAX name, in capitals.
            pub fn name(self) -> &'static str {
                match self {
                    $(LibFn::$variant => $name),*
                }
            }

            /// The kinds of the required arguments, then of the optional ones.
            fn kinds(self) -> (&'static [ArgKind], &'static [ArgKind]) {
                match self {
                    $(LibFn::$variant => (
                        &[$(ArgKind::$kind),*],
                        &[$($(ArgKind::$opt),+)?],
                    )),*
                }
            }

            pub fn returns(self) -> Returns {
                match self {
                    $(LibFn::$variant => Returns::$ret),*
                }
            }
        }
    };
}

library! {
    Abs = "ABS" (NumOrBlank) -> Num;
    Int = "INT" (NumOrBlank) -> Num;
    Round = "ROUND" (NumOrBlank, NumOrBlank) -> Num;
    Mod = "MOD" (Num, Num) -> Num;
    Upper = "UPPER" (Text) -> Text;
    Lower = "LOWER" (Text) -> Text;
    Len = "LEN" (Text) -> Num;
    Trim = "TRIM" (Text) -> Text;
    Left = "LEFT" (Text; Int) -> Text;
    Right = "RIGHT" (Text; Int) -> Text;
    Mid = "MID" (Text, Int, Int) -> Text;
}

impl LibFn {
    pub fn by_name(name: &str) -> Option<LibFn> {
        let upper = name.to_ascii_uppercase();
        LibFn::ALL.iter().copied().find(|f| f.name() == upper)
    }

    /// The fewest and most arguments it takes.
    pub fn arity(self) -> (usize, usize) {
        let (req, opt) = self.kinds();
        (req.len(), req.len() + opt.len())
    }

    /// How argument `i` is read.
    pub fn kind(self, i: usize) -> ArgKind {
        let (req, opt) = self.kinds();
        req.iter()
            .chain(opt)
            .nth(i)
            .copied()
            .unwrap_or(ArgKind::Num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_capitals() {
        for (i, f) in LibFn::ALL.iter().enumerate() {
            assert_eq!(f.name(), f.name().to_ascii_uppercase());
            assert!(LibFn::ALL[..i].iter().all(|g| g.name() != f.name()));
            assert_eq!(LibFn::by_name(&f.name().to_lowercase()), Some(*f));
        }
        assert_eq!(LibFn::Left.arity(), (1, 2));
        assert_eq!(LibFn::Left.kind(1), ArgKind::Int);
    }
}
