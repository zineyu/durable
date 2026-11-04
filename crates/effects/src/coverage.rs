//! 覆盖率：哪几类副作用真的发生过，以及声明覆盖却没出现的缺口。
//! 缺口怎么处理由使用方断言；类别清单在 `policy`。

use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::policy::{Effect, EffectScope};

/// 记录实际发生过的类别。使用方在写接口里对「已登记」调用 [`record`](Coverage::record)。
pub struct Coverage<E> {
    seen: Mutex<Vec<E>>,
}

impl<E: Effect> Coverage<E> {
    /// 还没有任何记录。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次发生。同一个类别记多次只保留第一次。
    ///
    /// 用 `Vec` 而不是位图：不引入「最多 64 类」这种人造上限。每次记录都伴随一次
    /// 落库，因此锁开销无关紧要。
    pub fn record(&self, effect: E) {
        let mut seen = self.seen();
        if !seen.contains(&effect) {
            seen.push(effect);
        }
    }

    /// 真的发生过的类别，每个一次，按首次发生的顺序。
    ///
    /// 名字沿用「已登记」这个证据强度：记录发生在写接口的 `Declared` 分支上。
    #[must_use]
    pub fn declared(&self) -> Vec<E> {
        self.seen().clone()
    }

    /// 声明覆盖却没出现过的类别名。为空才算矩阵有效。
    #[must_use]
    pub fn gaps(&self) -> Vec<&'static str> {
        let seen = self.seen();
        E::ALL
            .iter()
            .filter(|effect| effect.matrix_scope() == EffectScope::Covered)
            .filter(|effect| !seen.contains(effect))
            .map(|effect| effect.name())
            .collect()
    }

    fn seen(&self) -> MutexGuard<'_, Vec<E>> {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<E: Effect> Default for Coverage<E> {
    fn default() -> Self {
        Self {
            seen: Mutex::new(Vec::new()),
        }
    }
}
