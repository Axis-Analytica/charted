//! The library functions: plain scalar functions of their arguments (no filter context,
//! no tables), declared once here with their argument kinds.
//!
//! Each one has an arm in the SQL compiler (`charted_compiler::visual::lib_sql`), in
//! the reference interpreter (`charted_compiler::interp::lib_fn`) and in the Ossie
//! translator, all exhaustive matches, so adding a row here fails to build until all
//! three handle it. The arguments reach those arms already read as their kind says,
//! the same way on both sides, so blank handling can't drift between them.
//!
//! The kinds and the errors follow Power BI as the Service answered (conformance
//! probes, November 2026): text functions give blank for a blank text, whole-number
//! arguments round, MOD by zero and a negative LEFT length are errors.
//!
//! Descriptions and parameter names for the editor live in `catalog.rs`.

/// How an argument is read before the function sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// A number (TRUE is 1); blank counts as zero.
    Num,
    /// A number (TRUE is 1); a blank argument makes the result blank.
    NumOrBlank,
    /// A number (TRUE is 1); blank counts as zero, and then a result of zero is blank
    /// (Power BI's math functions: SQRT ( BLANK () ) is blank, EXP ( BLANK () ) is 1).
    /// Only for the first argument.
    NumBlank,
    /// A whole number: decimals are rounded (half away from zero), blank counts as zero.
    Int,
    /// Any value, as given (FIND's value for text not found, which may be blank).
    Any,
    /// Text; blank counts as empty text, numbers and dates as they're written.
    Text,
    /// Text, numbers and dates as they're written; a blank argument makes the result
    /// blank (Power BI's UPPER, LEN, LEFT, TRIM, MID…).
    TextOrBlank,
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
    Round = "ROUND" (NumOrBlank, Num) -> Num;
    Mod = "MOD" (NumOrBlank, Num) -> Num;
    Upper = "UPPER" (TextOrBlank) -> Text;
    Lower = "LOWER" (TextOrBlank) -> Text;
    Len = "LEN" (TextOrBlank) -> Num;
    Trim = "TRIM" (TextOrBlank) -> Text;
    Left = "LEFT" (TextOrBlank; Int) -> Text;
    Right = "RIGHT" (TextOrBlank; Int) -> Text;
    Mid = "MID" (TextOrBlank, Int, Int) -> Text;
    Value = "VALUE" (TextOrBlank) -> Num;
    PathLength = "PATHLENGTH" (Text) -> Num;
    PathItem = "PATHITEM" (Text, Int) -> Text;
    PathItemReverse = "PATHITEMREVERSE" (Text, Int) -> Text;
    PathContains = "PATHCONTAINS" (Text, Text) -> Bool;
    // Math (M11.6 batch 1).
    Ceiling = "CEILING" (NumBlank, Num) -> Num;
    Floor = "FLOOR" (NumBlank, Num) -> Num;
    IsoCeiling = "ISO.CEILING" (NumBlank; Num) -> Num;
    RoundUp = "ROUNDUP" (NumBlank, Num) -> Num;
    RoundDown = "ROUNDDOWN" (NumBlank, Num) -> Num;
    Trunc = "TRUNC" (NumBlank; Num) -> Num;
    MRound = "MROUND" (NumBlank, Num) -> Num;
    Even = "EVEN" (NumBlank) -> Num;
    Odd = "ODD" (NumBlank) -> Num;
    Sqrt = "SQRT" (NumBlank) -> Num;
    SqrtPi = "SQRTPI" (NumBlank) -> Num;
    Power = "POWER" (NumBlank, Num) -> Num;
    Exp = "EXP" (NumBlank) -> Num;
    Ln = "LN" (NumBlank) -> Num;
    Log = "LOG" (NumBlank; Num) -> Num;
    Log10 = "LOG10" (NumBlank) -> Num;
    Sign = "SIGN" (NumBlank) -> Num;
    Quotient = "QUOTIENT" (NumBlank, Num) -> Num;
    Fact = "FACT" (NumBlank) -> Num;
    Gcd = "GCD" (Int, Int) -> Num;
    Lcm = "LCM" (Int, Int) -> Num;
    Degrees = "DEGREES" (NumBlank) -> Num;
    Radians = "RADIANS" (NumBlank) -> Num;
    Sin = "SIN" (NumBlank) -> Num;
    Cos = "COS" (NumBlank) -> Num;
    Tan = "TAN" (NumBlank) -> Num;
    Cot = "COT" (NumBlank) -> Num;
    Asin = "ASIN" (NumBlank) -> Num;
    Acos = "ACOS" (NumBlank) -> Num;
    Atan = "ATAN" (NumBlank) -> Num;
    Acot = "ACOT" (NumBlank) -> Num;
    Sinh = "SINH" (NumBlank) -> Num;
    Cosh = "COSH" (NumBlank) -> Num;
    Tanh = "TANH" (NumBlank) -> Num;
    Coth = "COTH" (NumBlank) -> Num;
    Asinh = "ASINH" (NumBlank) -> Num;
    Acosh = "ACOSH" (NumBlank) -> Num;
    Atanh = "ATANH" (NumBlank) -> Num;
    Acoth = "ACOTH" (NumBlank) -> Num;
    Currency = "CURRENCY" (NumBlank) -> Num;
    // Text (M11.6 batch 2).
    Find = "FIND" (Text, Text; Int, Any) -> Num;
    Search = "SEARCH" (Text, Text; Int, Any) -> Num;
    ContainsString = "CONTAINSSTRING" (Text, Text) -> Bool;
    ContainsStringExact = "CONTAINSSTRINGEXACT" (Text, Text) -> Bool;
    Exact = "EXACT" (Text, Text) -> Bool;
    Replace = "REPLACE" (Text, Int, Int, Text) -> Text;
    Rept = "REPT" (Text, Int) -> Text;
    Fixed = "FIXED" (Num; Int, Num) -> Text;
    Unichar = "UNICHAR" (NumOrBlank) -> Text;
    Unicode = "UNICODE" (TextOrBlank) -> Num;
    CombineValues = "COMBINEVALUES" (Text, Text, Text; Text, Text, Text, Text, Text, Text) -> Text;
    Rand = "RAND" () -> Num;
    RandBetween = "RANDBETWEEN" (Int, Int) -> Num;
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
