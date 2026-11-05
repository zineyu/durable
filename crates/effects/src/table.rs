//! 类别清单的单一来源：一份声明同时生成枚举、全量清单与稳定标签。
//! 宏只在这里定义，使用者以 `effects::effect_table!` 展开；生成物是普通的枚举与常量，
//! 查文档、跳转与匹配都不需要先展开宏。重放策略与覆盖表态不在这里：它们由使用者手写，
//! 并且不带兜底分支（ADR 0062）。

/// 由一份变体清单生成枚举、全量清单与稳定标签。
///
/// 清单只出现一次，因此不存在「清单与变体集合不一致」这个形状。`replay` 与
/// `matrix_scope` 仍由使用者手写并且不带兜底分支：新增一类时编译器在那两处报错。
#[macro_export]
macro_rules! effect_table {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident => $label:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            /// 全部变体，按声明顺序。清单只在这里出现一次。
            pub const ALL_CLASSES: &'static [Self] = &[$(Self::$variant),+];

            /// 稳定字面量。日志、告警与测试失败信息都据它说话。
            #[must_use]
            pub const fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label),+
                }
            }
        }
    };
}
