//! 矩阵契约的单元测试：逐点完备、点数对账、失败互不遮挡、覆盖面两条规则，以及
//! 「把别处覆盖改成矩阵覆盖就立刻红」这条常驻的反向验证。
//! 崩溃点是本地结构体，类别用 `support::Probe` 与本地类别表，因此这些用例不依赖领域类型。

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use effects::coverage::Coverage;
use effects::matrix::{Checks, CrashPoint, Finding, Kind, Matrix, coverage_findings};
use effects::policy::{Effect, EffectScope, Evidence, Replay};
use support::Probe;

/// 每个点都要应答的坐标。清单只有一处，测试与库都据它核对完备性。
const COORDS: &[&str] = &["收敛", "历史保留"];

/// 一个崩溃点：标签，以及它是否失败。
#[derive(Clone)]
struct Point {
    label: &'static str,
    fails: bool,
}

impl Point {
    const fn new(label: &'static str, fails: bool) -> Self {
        Self { label, fails }
    }
}

impl CrashPoint for Point {
    fn label(&self) -> String {
        self.label.to_owned()
    }
}

/// 一个点对全部坐标的结论。
fn answer(point: &Point) -> Checks {
    let mut checks = Checks::new();
    for check in COORDS {
        let outcome = if point.fails {
            Err(format!("{}：不成立", point.label))
        } else {
            Ok(())
        };
        checks.record(check, outcome);
    }
    checks
}

/// 三个点、中间一个失败：另外两个照常被跑，失败全部留下。
#[tokio::test]
async fn one_failing_point_does_not_hide_the_others() {
    let visited = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&visited);
    let points = vec![
        Point::new("p1", false),
        Point::new("p2", true),
        Point::new("p3", false),
    ];
    let report = Matrix::new(points, 3, COORDS)
        .sweep(async move |point: Point| {
            seen.fetch_add(1, Ordering::SeqCst);
            answer(&point)
        })
        .await;

    assert_eq!(
        visited.load(Ordering::SeqCst),
        3,
        "一个点失败不能打断其它点：{report}"
    );
    assert_eq!(report.len(), COORDS.len(), "只报失败的那两条：{report}");
    for finding in report.findings() {
        assert_eq!(finding.kind(), Kind::Failed, "来由是断言不成立：{finding}");
        assert_eq!(finding.point(), Some("p2"), "失败只出在 p2：{finding}");
    }
}

/// 一个点什么也没答：每条坐标各成一行「未应答」，而不是安静地少测。
#[tokio::test]
async fn a_point_that_answers_nothing_is_unchecked() {
    let report = Matrix::new(vec![Point::new("p1", false)], 1, COORDS)
        .sweep(async |_point: Point| Checks::new())
        .await;

    assert_eq!(report.len(), COORDS.len(), "每条坐标各一行：{report}");
    for finding in report.findings() {
        assert_eq!(finding.kind(), Kind::Unchecked, "来由是未应答：{finding}");
        assert_eq!(finding.point(), Some("p1"), "点位在报告里：{finding}");
    }
}

/// 少答一条坐标：只有那一条成行，其余的通过不受影响。
#[tokio::test]
async fn a_missing_coordinate_is_unchecked() {
    let report = Matrix::new(vec![Point::new("p1", false)], 1, COORDS)
        .sweep(async |_point: Point| {
            let mut checks = Checks::new();
            checks.pass("收敛");
            checks
        })
        .await;

    assert_eq!(report.len(), 1, "只缺一条：{report}");
    for finding in report.findings() {
        assert_eq!(finding.kind(), Kind::Unchecked, "来由是未应答：{finding}");
        assert_eq!(finding.check(), Some("历史保留"), "缺的是哪一条：{finding}");
    }
}

/// 点表比使用方数出的少：点数对账成一行，说明矩阵少测了。
#[tokio::test]
async fn a_point_table_shorter_than_the_count_is_reported() {
    let report = Matrix::new(vec![Point::new("p1", false)], 3, COORDS)
        .sweep(async |point: Point| answer(&point))
        .await;

    let missing: Vec<&str> = report
        .findings()
        .iter()
        .filter(|finding| finding.kind() == Kind::PointMissing)
        .map(Finding::detail)
        .collect();
    assert_eq!(missing.len(), 1, "点数不符恰好一行：{report}");
    assert!(
        missing
            .first()
            .is_some_and(|detail| detail.contains("1 个")),
        "说明里带实际点数：{report}"
    );
}

/// 断言挂在不存在的点上：那是点表以外的结论，报告点名它。
#[tokio::test]
async fn a_recorded_point_outside_the_table_is_reported() {
    let mut matrix = Matrix::new(vec![Point::new("p1", false)], 1, COORDS);
    matrix.record(&Point::new("p9", false), answer(&Point::new("p9", false)));
    let report = matrix.report();

    let stray: Vec<&str> = report
        .findings()
        .iter()
        .filter(|finding| finding.kind() == Kind::Stray)
        .filter_map(|finding| finding.point())
        .collect();
    assert_eq!(stray, vec!["p9"], "点表以外的点被点名：{report}");
}

/// 场景级失败与逐点结论进同一份报告，顺序固定：先逐点，再场景级。
#[tokio::test]
async fn a_scenario_level_failure_is_kept_next_to_the_points() {
    let mut matrix = Matrix::new(vec![Point::new("p1", false)], 1, COORDS);
    matrix.record(&Point::new("p1", true), answer(&Point::new("p1", true)));
    let mut report = matrix.report();
    report.extend([Finding::new(
        Kind::PointMissing,
        "场景",
        "至少一个点应落在 run 中间",
    )]);

    assert_eq!(report.len(), COORDS.len() + 1, "逐点加场景级：{report}");
    assert_eq!(
        report.findings().last().map(Finding::kind),
        Some(Kind::PointMissing),
        "场景级结论排在最后：{report}"
    );
}

/// 声明矩阵覆盖的类别没有出现：缺口点名类别名。
#[test]
fn a_covered_category_that_never_happened_is_named() {
    let coverage = Coverage::<Probe>::new();
    coverage.record(Probe::Redo);
    coverage.record(Probe::Elsewhere);

    let findings = coverage_findings(&coverage);
    assert_eq!(findings.len(), 2, "缺两个类别：{findings:?}");
    for finding in &findings {
        assert_eq!(finding.kind(), Kind::EffectMissing, "来由是覆盖缺口");
    }
    let details: Vec<&str> = findings.iter().map(Finding::detail).collect();
    assert!(
        details
            .iter()
            .any(|detail| detail.contains("probe_give_up")),
        "点名缺少的类别：{details:?}"
    );
}

/// 声明别处覆盖却没有写理由：这条声明无法核对，仍然算缺口。
#[test]
fn an_elsewhere_category_without_a_reason_is_named() {
    let coverage = Coverage::<Thin>::new();
    coverage.record(Thin::Covered);

    let findings = coverage_findings(&coverage);
    assert_eq!(findings.len(), 1, "只有理由为空那一条：{findings:?}");
    assert_eq!(
        findings.first().map(Finding::kind),
        Some(Kind::ReasonEmpty),
        "来由是理由为空"
    );
}

/// 反向验证：同一份观测，把「别处覆盖」改成「矩阵覆盖」之后报告立刻红。
///
/// 这是 issue 里那条判据的常驻形态：它不依赖任何真实场景，也不依赖手工翻转。
#[test]
fn flipping_an_elsewhere_category_to_covered_turns_the_same_run_red() {
    let honest = Coverage::<Honest>::new();
    honest.record(Honest::Seen);
    let green = coverage_findings(&honest);
    assert!(green.is_empty(), "别处覆盖不是缺口：{green:?}");

    let claimed = Coverage::<Claimed>::new();
    claimed.record(Claimed::Seen);
    let red = coverage_findings(&claimed);
    assert_eq!(red.len(), 1, "改成矩阵覆盖之后立刻红：{red:?}");
    assert_eq!(red.first().map(Finding::kind), Some(Kind::EffectMissing));
    assert!(
        red.first()
            .is_some_and(|finding| finding.detail().contains("claimed_deferred")),
        "点名那个类别：{red:?}"
    );
}

/// 只用来断言「别处覆盖」的类别：一个真的出现过，一个声明别处覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Honest {
    Seen,
    Deferred,
}

impl Effect for Honest {
    const ALL: &'static [Self] = &[Self::Seen, Self::Deferred];

    fn name(self) -> &'static str {
        match self {
            Self::Seen => "honest_seen",
            Self::Deferred => "honest_deferred",
        }
    }

    fn replay(self, _evidence: Evidence) -> Replay {
        Replay::Redo
    }

    fn matrix_scope(self) -> EffectScope {
        match self {
            Self::Seen => EffectScope::Covered,
            Self::Deferred => EffectScope::Elsewhere("由另一个测试目标覆盖"),
        }
    }
}

/// 与 [`Honest`] 同一份观测，只把「别处覆盖」那一行改成「矩阵覆盖」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Claimed {
    Seen,
    Deferred,
}

impl Effect for Claimed {
    const ALL: &'static [Self] = &[Self::Seen, Self::Deferred];

    fn name(self) -> &'static str {
        match self {
            Self::Seen => "claimed_seen",
            Self::Deferred => "claimed_deferred",
        }
    }

    fn replay(self, _evidence: Evidence) -> Replay {
        Replay::Redo
    }

    fn matrix_scope(self) -> EffectScope {
        match self {
            Self::Seen | Self::Deferred => EffectScope::Covered,
        }
    }
}

/// 理由为空的类别表：只有这一个取值，用来钉住「空理由算缺口」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Thin {
    Covered,
    NoReason,
}

impl Effect for Thin {
    const ALL: &'static [Self] = &[Self::Covered, Self::NoReason];

    fn name(self) -> &'static str {
        match self {
            Self::Covered => "thin_covered",
            Self::NoReason => "thin_no_reason",
        }
    }

    fn replay(self, _evidence: Evidence) -> Replay {
        Replay::Redo
    }

    fn matrix_scope(self) -> EffectScope {
        match self {
            Self::Covered => EffectScope::Covered,
            Self::NoReason => EffectScope::Elsewhere("   "),
        }
    }
}

/// 反方向也要出声：一个点给出了没有登记在坐标清单里的结论，报告点名它。
///
/// 少了这条，「忘了一条断言」只是从报告里消失，正是本条要消灭的病。
#[tokio::test]
async fn a_coordinate_that_is_not_declared_is_named() {
    let report = Matrix::new(vec![Point::new("p1", false)], 1, COORDS)
        .sweep(async |_point: Point| {
            let mut checks = Checks::new();
            for check in COORDS {
                checks.pass(check);
            }
            checks.pass("没有登记的坐标");
            checks
        })
        .await;

    assert_eq!(report.len(), 1, "多出来的坐标恰好一行：{report}");
    for finding in report.findings() {
        assert_eq!(
            finding.kind(),
            Kind::Unregistered,
            "来由是坐标未登记：{finding}"
        );
        assert_eq!(
            finding.check(),
            Some("没有登记的坐标"),
            "点名那条坐标：{finding}"
        );
    }
}
