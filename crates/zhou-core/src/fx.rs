//! 定点数：带小数的量一律以千分之一为单位存成整数（1.5 存为 1500）。
//! 结算只用整数运算，在哪台机器上算结果都一样，也不会累积浮点误差。
//!
//! 哪里都是这个整数，不做换算：配置和剧本的 toml、存档、指令、发给前端的报告和配置。
//! 只在给人看的文字里（事件、错误消息）显示成小数。

use std::fmt;
use std::iter::{Product, Sum};
use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

use serde::{Deserialize, Deserializer, Serialize};

/// 1 等于多少个最小单位
pub const SCALE: i64 = 1000;

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Fx(pub i64);

impl Fx {
    pub const ZERO: Fx = Fx(0);
    pub const ONE: Fx = Fx(SCALE);
    /// 最小单位：千分之一
    pub const EPS: Fx = Fx(1);
    pub const MAX: Fx = Fx(i64::MAX);

    /// 整数 n
    pub const fn int(n: i64) -> Fx {
        Fx(n * SCALE)
    }

    /// 千分之 n
    pub const fn milli(n: i64) -> Fx {
        Fx(n)
    }

    /// n / d，四舍五入到千分之一。d 为 0 时得 0
    pub fn ratio(n: i64, d: i64) -> Fx {
        Fx(div_round(n as i128 * SCALE as i128, d as i128))
    }

    /// 百分之 n
    pub const fn percent(n: i64) -> Fx {
        Fx(n * SCALE / 100)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }

    /// 向下取整
    pub const fn floor(self) -> i64 {
        self.0.div_euclid(SCALE)
    }

    /// 向上取整
    pub const fn ceil(self) -> i64 {
        -(-self.0).div_euclid(SCALE)
    }

    /// 四舍五入
    pub fn round(self) -> i64 {
        div_round(self.0 as i128, SCALE as i128)
    }

    /// 向下取整成人数之类的非负整数，负数得 0
    pub fn floor_u32(self) -> u32 {
        self.floor().clamp(0, u32::MAX as i64) as u32
    }

    pub fn round_u32(self) -> u32 {
        self.round().clamp(0, u32::MAX as i64) as u32
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub const fn abs(self) -> Fx {
        Fx(self.0.abs())
    }

    /// 乘以 n/d，中间不丢精度
    pub fn mul_ratio(self, n: i64, d: i64) -> Fx {
        Fx(div_round(self.0 as i128 * n as i128, d as i128))
    }
}

/// 四舍五入的整数除法（远离零取舍）；除数为 0 时得 0
fn div_round(n: i128, d: i128) -> i64 {
    if d == 0 {
        return 0;
    }
    let q = n / d;
    let r = n % d;
    let q = if 2 * r.abs() >= d.abs() {
        q + if (n < 0) == (d < 0) { 1 } else { -1 }
    } else {
        q
    };
    q.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

impl Add for Fx {
    type Output = Fx;
    fn add(self, o: Fx) -> Fx {
        Fx(self.0.saturating_add(o.0))
    }
}

impl Sub for Fx {
    type Output = Fx;
    fn sub(self, o: Fx) -> Fx {
        Fx(self.0.saturating_sub(o.0))
    }
}

impl Neg for Fx {
    type Output = Fx;
    fn neg(self) -> Fx {
        Fx(-self.0)
    }
}

impl AddAssign for Fx {
    fn add_assign(&mut self, o: Fx) {
        *self = *self + o;
    }
}

impl SubAssign for Fx {
    fn sub_assign(&mut self, o: Fx) {
        *self = *self - o;
    }
}

/// 定点数相乘，四舍五入到千分之一
impl Mul for Fx {
    type Output = Fx;
    fn mul(self, o: Fx) -> Fx {
        Fx(div_round(self.0 as i128 * o.0 as i128, SCALE as i128))
    }
}

impl MulAssign for Fx {
    fn mul_assign(&mut self, o: Fx) {
        *self = *self * o;
    }
}

/// 定点数相除，四舍五入到千分之一；除数为 0 时得 0
impl Div for Fx {
    type Output = Fx;
    // 定点数相除要先乘上 SCALE
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn div(self, o: Fx) -> Fx {
        Fx(div_round(self.0 as i128 * SCALE as i128, o.0 as i128))
    }
}

/// 引用参与运算（迭代 BTreeMap 时拿到的是 &Fx）
macro_rules! ref_ops {
    ($($tr:ident $f:ident),*) => {$(
        impl $tr<&Fx> for Fx {
            type Output = Fx;
            fn $f(self, o: &Fx) -> Fx {
                $tr::$f(self, *o)
            }
        }
        impl $tr<Fx> for &Fx {
            type Output = Fx;
            fn $f(self, o: Fx) -> Fx {
                $tr::$f(*self, o)
            }
        }
        impl $tr<&Fx> for &Fx {
            type Output = Fx;
            fn $f(self, o: &Fx) -> Fx {
                $tr::$f(*self, *o)
            }
        }
    )*};
}
ref_ops!(Add add, Sub sub, Mul mul, Div div);

macro_rules! int_ops {
    ($($t:ty),*) => {$(
        /// 乘以整数（人数、回合数……），不丢精度
        impl Mul<$t> for Fx {
            type Output = Fx;
            fn mul(self, n: $t) -> Fx {
                Fx((self.0 as i128 * n as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64)
            }
        }
        impl Mul<Fx> for $t {
            type Output = Fx;
            fn mul(self, f: Fx) -> Fx {
                f * self
            }
        }
        /// 除以整数，四舍五入；除数为 0 时得 0
        impl Div<$t> for Fx {
            type Output = Fx;
            fn div(self, n: $t) -> Fx {
                Fx(div_round(self.0 as i128, n as i128))
            }
        }
        impl From<$t> for Fx {
            fn from(n: $t) -> Fx {
                Fx((n as i128 * SCALE as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64)
            }
        }
    )*};
}
int_ops!(i32, i64, u32, u64, usize);

impl Sum for Fx {
    fn sum<I: Iterator<Item = Fx>>(iter: I) -> Fx {
        iter.fold(Fx::ZERO, |a, b| a + b)
    }
}

impl Product for Fx {
    fn product<I: Iterator<Item = Fx>>(iter: I) -> Fx {
        iter.fold(Fx::ONE, |a, b| a * b)
    }
}

impl<'a> Sum<&'a Fx> for Fx {
    fn sum<I: Iterator<Item = &'a Fx>>(iter: I) -> Fx {
        iter.fold(Fx::ZERO, |a, b| a + *b)
    }
}

/// 显示成小数，去掉末尾的 0：1500 → 1.5，2000 → 2。
/// 指定精度时四舍五入到那么多位：`{:.0}` 显示整数，`{:.1}` 一位小数（最多 3 位）
impl fmt::Display for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = f.precision().map_or(3, |p| p.min(3));
        let unit = 10i64.pow(3 - digits as u32);
        let v = div_round(self.0 as i128, unit as i128);
        let sign = if v < 0 { "-" } else { "" };
        let a = v.unsigned_abs();
        let base = 10u64.pow(digits as u32);
        let (int, frac) = (a / base, a % base);
        if f.precision().is_some() {
            if digits == 0 {
                write!(f, "{sign}{int}")
            } else {
                write!(f, "{sign}{int}.{frac:0digits$}")
            }
        } else if frac == 0 {
            write!(f, "{sign}{int}")
        } else {
            let s = format!("{frac:03}");
            write!(f, "{sign}{int}.{}", s.trim_end_matches('0'))
        }
    }
}

impl fmt::Debug for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// 只收整数（千分之一为单位），收到小数报错
impl<'de> Deserialize<'de> for Fx {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Fx, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Fx;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("整数（千分之一为单位，1.5 写作 1500）")
            }
            fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<Fx, E> {
                Ok(Fx(n))
            }
            fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<Fx, E> {
                i64::try_from(n)
                    .map(Fx)
                    .map_err(|_| E::custom(format!("数太大：{n}")))
            }
        }
        d.deserialize_i64(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        assert_eq!(Fx::int(3) * Fx::milli(500), Fx::milli(1500));
        assert_eq!(Fx::int(1) / Fx::int(3), Fx::milli(333));
        assert_eq!(Fx::int(2) / Fx::int(3), Fx::milli(667));
        assert_eq!(Fx::milli(1500) * 3u32, Fx::milli(4500));
        assert_eq!(Fx::milli(2500).round(), 3);
        assert_eq!(Fx::milli(-2500).round(), -3);
        assert_eq!(Fx::milli(2500).floor(), 2);
        assert_eq!(Fx::milli(-2500).floor(), -3);
        assert_eq!(Fx::milli(2001).ceil(), 3);
        assert_eq!(Fx::ratio(1, 4), Fx::milli(250));
        assert_eq!(Fx::int(1) / Fx::ZERO, Fx::ZERO);
        assert_eq!(Fx::milli(1500).to_string(), "1.5");
        assert_eq!(Fx::milli(-50).to_string(), "-0.05");
        assert_eq!(Fx::int(7).to_string(), "7");
        assert_eq!(format!("{:.0}", Fx::milli(33_500)), "34");
        assert_eq!(format!("{:.1}", Fx::milli(2_250)), "2.3");
        assert_eq!(format!("{:.2}", Fx::milli(-50)), "-0.05");
    }

    #[test]
    fn reads_integers_only() {
        #[derive(Deserialize)]
        struct T {
            a: Fx,
        }
        let t: T = toml::from_str("a = 1500").unwrap();
        assert_eq!(t.a, Fx::milli(1500));
        assert!(toml::from_str::<T>("a = 1.5").is_err());
    }
}
