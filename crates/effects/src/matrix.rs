//! 崩溃点契约与遍历：把「每个点各跑一遍检查」「一个点失败不打断其它点」与「覆盖面缺口」
//! 汇成一份可显示、可断言的报告。
//! 崩溃点从哪来（提交点副本、进程快照、注入的故障）归使用方，重启之后断言什么也归使用方；
//! 库只规定四件事：每个点有稳定标签、每个点应答同一组坐标、失败是可收集的数据、
//! 点数与覆盖面的缺口各成一行。它不执行副作用、不持有存储、不认识任何业务类型、
//! 不定义错误类型、不写日志、不计时。类别表在 `policy`，覆盖率在 `coverage`。

use core::fmt::{self, Display, Formatter};

use crate::coverage::Coverage;
use crate::policy::{Effect, EffectScope};

/// 一个崩溃点的身份。标签只由点自身决定，因此报告可以跨运行比对。
pub trait CrashPoint {
    /// 稳定标签。同一个点每次遍历得到同一串。
    fn label(&self) -> String;
}

/// 报告一行的来由。取值即「为什么红」，因此报告不必靠文案区分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 断言不成立。
    Failed,
    /// 这个点没有应答这条坐标。
    Unchecked,
    /// 断言挂在一个不在点表里的点上。
    Stray,
    /// 点表与已提交次数不符：矩阵少测或没测。
    PointMissing,
    /// 声明矩阵覆盖的类别从未出现。
    EffectMissing,
    /// 声明别处覆盖的类别没有写理由。
    ReasonEmpty,
    /// 一个点给出了没有登记在坐标清单里的结论：那条断言不会被核对。
    Unregistered,
}

impl Kind {
    /// 稳定字面量。报告与告警都据它说话。
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Failed => "不成立",
            Self::Unchecked => "未应答",
            Self::Stray => "点表以外",
            Self::PointMissing => "点数不符",
            Self::EffectMissing => "覆盖缺口",
            Self::ReasonEmpty => "理由为空",
            Self::Unregistered => "坐标未登记",
        }
    }
}

/// 报告里的一行：来由、点、坐标与说明。`point` 为空表示场景级结论或覆盖面缺口。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    kind: Kind,
    point: Option<String>,
    check: Option<&'static str>,
    detail: String,
}

impl Finding {
    /// 不挂在某个点上的失败：场景级结论与覆盖面缺口。
    #[must_use]
    pub fn new(kind: Kind, check: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind,
            point: None,
            check: Some(check),
            detail: detail.into(),
        }
    }

    /// 某个点上的某条坐标不成立。
    fn failed_at(point: &str, check: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind: Kind::Failed,
            point: Some(point.to_owned()),
            check: Some(check),
            detail: detail.into(),
        }
    }

    /// 为什么红。
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// 哪一个崩溃点。`None` 表示这条结论不挂在任何点上。
    #[must_use]
    pub fn point(&self) -> Option<&str> {
        self.point.as_deref()
    }

    /// 哪一条坐标。`None` 表示这条结论不是逐点断言给出的。
    #[must_use]
    pub fn check(&self) -> Option<&'static str> {
        self.check
    }

    /// 人读的说明。
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl Display for Finding {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(point) = &self.point {
            write!(formatter, "{point} · ")?;
        }
        if let Some(check) = self.check {
            write!(formatter, "{check} · ")?;
        }
        write!(formatter, "{}：{}", self.kind.label(), self.detail)
    }
}

/// 一个崩溃点上给出的结论。
///
/// 按坐标记录而不是只记「通过或失败」，因此「少答一条坐标」能被发现，并在报告里成为
/// 一行 [`Kind::Unchecked`]。同一个坐标记两次以上时失败优先：后写的一次通过不能把失败抹掉。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Checks {
    verdicts: Vec<(&'static str, Option<String>)>,
}

impl Checks {
    /// 还没有任何结论。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 这条坐标成立。
    pub fn pass(&mut self, check: &'static str) {
        self.verdicts.push((check, None));
    }

    /// 这条坐标不成立，附说明。
    pub fn fail(&mut self, check: &'static str, detail: impl Into<String>) {
        self.verdicts.push((check, Some(detail.into())));
    }

    /// 由结论取值收口：成立记一次通过，不成立把说明留下。
    pub fn record(&mut self, check: &'static str, outcome: Result<(), String>) {
        match outcome {
            Ok(()) => self.pass(check),
            Err(detail) => self.fail(check, detail),
        }
    }

    /// 已经应答过的坐标名，按记录顺序。
    fn answered(&self) -> impl Iterator<Item = &'static str> {
        self.verdicts.iter().map(|(check, _)| *check)
    }

    /// 某条坐标的结论。
    fn verdict(&self, check: &'static str) -> Answer<'_> {
        let mut answered = false;
        for (held, detail) in &self.verdicts {
            if *held != check {
                continue;
            }
            if let Some(detail) = detail {
                return Answer::Failed(detail);
            }
            answered = true;
        }
        if answered {
            Answer::Passed
        } else {
            Answer::Unanswered
        }
    }
}

/// 一条坐标的三种可能结论。三态用枚举而不是 `Option<Option<_>>`。
enum Answer<'a> {
    /// 这个点没有应答这条坐标。
    Unanswered,
    /// 应答了，成立。
    Passed,
    /// 应答了，不成立。
    Failed(&'a str),
}

/// 一次遍历的点表、每个点必须应答的坐标，以及逐点与场景级的结论。
///
/// 点数对账在报告里是内建结构而不是可选断言：点表比已提交次数少一个，就意味着矩阵静默
/// 少测了一个崩溃点，那正是「全部副本等于全部崩溃点」这条前提被打破的形状。
#[derive(Debug)]
pub struct Matrix<P> {
    points: Vec<P>,
    expected: usize,
    per_point: &'static [&'static str],
    recorded: Vec<(String, Checks)>,
}

impl<P: CrashPoint + Clone> Matrix<P> {
    /// 用点表、使用方独立的计数与逐点坐标清单建立一次遍历。
    ///
    /// `expected` 是使用方自己数出来的数目（在我们的场景里是已提交次数）。`per_point` 是
    /// 每个点都必须应答的坐标清单；少答一条即报告里的一行 [`Kind::Unchecked`]。
    #[must_use]
    pub fn new(points: Vec<P>, expected: usize, per_point: &'static [&'static str]) -> Self {
        Self {
            points,
            expected,
            per_point,
            recorded: Vec::new(),
        }
    }

    /// 遍历全部点并汇总成报告。
    ///
    /// 一个点的失败不打断其它点：检查返回结论而不返回错误，本函数没有提前返回、没有 `?`、
    /// 也不 panic。报告由调用方断言，因此本函数不给库引入 panic 路径。
    pub async fn sweep<F>(mut self, mut check: F) -> Report
    where
        F: AsyncFnMut(P) -> Checks,
    {
        for point in self.points.clone() {
            let label = point.label();
            let checks = check(point).await;
            self.recorded.push((label, checks));
        }
        self.report()
    }

    /// 收下一个点的结论。手工逐点时用，`sweep` 走同一条路。
    pub fn record(&mut self, point: &P, checks: Checks) {
        self.recorded.push((point.label(), checks));
    }

    /// 汇总成报告。
    ///
    /// 行的顺序固定：点数对账、点表以外的断言、逐点逐坐标。跨点才能得出的结论与覆盖面
    /// 缺口由调用方用 [`Report::extend`] 追加。
    #[must_use]
    pub fn report(self) -> Report {
        let labels: Vec<String> = self.points.iter().map(CrashPoint::label).collect();
        let mut findings = Vec::new();
        self.audit_count(&mut findings);
        self.audit_stray(&labels, &mut findings);
        for label in &labels {
            self.audit_point(label, &mut findings);
        }
        Report { findings }
    }

    fn audit_count(&self, findings: &mut Vec<Finding>) {
        if self.points.len() == self.expected {
            return;
        }
        findings.push(Finding::new(
            Kind::PointMissing,
            "点数",
            format!(
                "枚举出 {} 个崩溃点，使用方数出 {} 个：矩阵少测或没测",
                self.points.len(),
                self.expected
            ),
        ));
    }

    fn audit_stray(&self, labels: &[String], findings: &mut Vec<Finding>) {
        for (label, _) in &self.recorded {
            if labels.contains(label) {
                continue;
            }
            findings.push(Finding {
                kind: Kind::Stray,
                point: Some(label.clone()),
                check: None,
                detail: "这个点不在点表里，它的结论不会被逐点核对".to_owned(),
            });
        }
    }

    fn audit_point(&self, label: &str, findings: &mut Vec<Finding>) {
        let checks = self
            .recorded
            .iter()
            .find(|(held, _)| held == label)
            .map(|(_, checks)| checks);
        for check in self.per_point {
            let finding = match checks {
                None => Some(unanswered(label, check)),
                Some(checks) => match checks.verdict(check) {
                    Answer::Unanswered => Some(unanswered(label, check)),
                    Answer::Passed => None,
                    Answer::Failed(detail) => Some(Finding::failed_at(label, check, detail)),
                },
            };
            if let Some(finding) = finding {
                findings.push(finding);
            }
        }
        let Some(checks) = checks else {
            return;
        };
        for check in checks.answered() {
            if self.per_point.contains(&check) {
                continue;
            }
            findings.push(Finding {
                kind: Kind::Unregistered,
                point: Some(label.to_owned()),
                check: Some(check),
                detail: "这条结论没有登记在坐标清单里，因此不会被逐点核对".to_owned(),
            });
        }
    }
}

/// 这个点没有应答这条坐标。
fn unanswered(label: &str, check: &'static str) -> Finding {
    Finding {
        kind: Kind::Unchecked,
        point: Some(label.to_owned()),
        check: Some(check),
        detail: "这个点没有应答这条坐标".to_owned(),
    }
}

/// 一次遍历的结论。为空才算有效。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    findings: Vec<Finding>,
}

impl Report {
    /// 追加不挂在任何点上的失败：跨点才能得出的结论与覆盖面缺口。
    pub fn extend(&mut self, findings: impl IntoIterator<Item = Finding>) {
        self.findings.extend(findings);
    }

    /// 并入另一份报告。多个场景的结果因此可以在同一处判分。
    pub fn merge(&mut self, other: Self) {
        self.findings.extend(other.findings);
    }
    /// 一条违例都没有。
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// 全部违例，按生成的顺序。
    #[must_use]
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// 违例条数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// 没有违例。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

impl Display for Report {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if self.findings.is_empty() {
            return formatter.write_str("矩阵全绿");
        }
        writeln!(formatter, "矩阵有 {} 条违例：", self.findings.len())?;
        for finding in &self.findings {
            writeln!(formatter, "  {finding}")?;
        }
        Ok(())
    }
}

/// 核对整张类别表：声明覆盖却没有出现的类别，与声明别处覆盖却没有写理由的类别，各成一行。
///
/// 「没覆盖」与「忘了覆盖」在结果上一样，因此两者都算报告里的一行。别处覆盖的类别不是
/// 缺口，但空理由让这条声明无法核对，仍然算缺口。
#[must_use]
pub fn coverage_findings<E: Effect>(coverage: &Coverage<E>) -> Vec<Finding> {
    let seen = coverage.declared();
    let mut findings = Vec::new();
    for effect in E::ALL {
        match effect.matrix_scope() {
            EffectScope::Covered => {
                if !seen.contains(effect) {
                    findings.push(Finding::new(
                        Kind::EffectMissing,
                        "覆盖",
                        format!("类别 {} 声明矩阵覆盖，但没有一个场景走到它", effect.name()),
                    ));
                }
            }
            EffectScope::Elsewhere(reason) => {
                if reason.trim().is_empty() {
                    findings.push(Finding::new(
                        Kind::ReasonEmpty,
                        "覆盖",
                        format!("类别 {} 声明别处覆盖，但没有写理由", effect.name()),
                    ));
                }
            }
        }
    }
    findings
}
