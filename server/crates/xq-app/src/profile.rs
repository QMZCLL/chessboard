//! 能力画像：由学习事件（见 `learning`）统计，回答三个问题——最近反复错在哪里、今天最值得练什么、
//! 是真的掌握了还是只记住了答案。全部由程序按次数给出，写明依据，不显示百分比或分数（没有校准过）；
//! 作答还不多时如实说明，不下结论。
//!
//! - 维度：棋谱中这一着的规则主题（与卡片的主题同一套判断），以及按子力粗分的对局阶段。
//!   数据还不多时主题与阶段的交叉格子大多是空的，所以分开统计。
//! - “作答”不含比较中与无法评价的局面决策；“没有独立选对”包括没有选对、直接看答案与看了提示后选对。
//! - “最近”是最近 30 天；“隔天以后”是距同一局面上一次作答 20 小时以上。

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;
use xq_core::PieceKind;

use crate::learning::Confidence;
use crate::learning::EventOutcome;
use crate::learning::LearningEvent;
use crate::topics::Theme;

/// “最近”的范围。
pub const RECENT_DAYS: i64 = 30;
const DAY_MS: i64 = 86_400_000;
/// 距同一局面上一次作答超过这个时间，算“隔天以后”再做。
pub const SPACED_MS: i64 = 20 * 3_600_000;
/// 作答少于这个次数时，画像只说明作答还不多。
pub const MIN_ANSWERED: u32 = 5;
/// 一个主题最近至少有这么多次作答，才判断它是不是反复出错的地方。
pub const MIN_THEME: u32 = 4;
/// 推荐练习一个主题时，至少要有这么多张卡片。
pub const MIN_CARDS: usize = 2;

/// 按子力粗分的对局阶段（车马炮的总数：开局 11 个以上，残局 6 个以下）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Phase {
    Opening,
    Middlegame,
    Endgame,
}

impl Phase {
    pub const ALL: [Phase; 3] = [Phase::Opening, Phase::Middlegame, Phase::Endgame];

    pub fn label(self) -> &'static str {
        match self {
            Phase::Opening => "开局",
            Phase::Middlegame => "中局",
            Phase::Endgame => "残局",
        }
    }

    /// 局面的阶段；局面无法读取时为空。
    pub fn of(fen: &str) -> Option<Phase> {
        let pos = xq_core::fen::parse_position(fen).ok()?;
        let majors = pos.board.pieces().filter(|(_, p)| matches!(p.kind, PieceKind::Rook | PieceKind::Horse | PieceKind::Cannon)).count();
        Some(match majors {
            11.. => Phase::Opening,
            0..=6 => Phase::Endgame,
            _ => Phase::Middlegame,
        })
    }
}

/// 作答的次数（不含比较中与无法评价）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Counts {
    pub answered: u32,
    pub independent: u32,
    pub assisted: u32,
    pub missed: u32,
    pub revealed: u32,
    /// 很有把握却没有选对。
    pub confident_misses: u32,
}

impl Counts {
    fn add(&mut self, e: &LearningEvent) {
        let counted = match e.outcome {
            EventOutcome::Independent => &mut self.independent,
            EventOutcome::Assisted => &mut self.assisted,
            EventOutcome::Missed => &mut self.missed,
            EventOutcome::Revealed => &mut self.revealed,
            EventOutcome::Pending | EventOutcome::Ungraded => return,
        };
        *counted += 1;
        self.answered += 1;
        if e.outcome == EventOutcome::Missed && e.confidence == Some(Confidence::Sure) {
            self.confident_misses += 1;
        }
    }

    /// 没有独立选对的次数。
    pub fn not_independent(&self) -> u32 { self.assisted + self.missed + self.revealed }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ThemeStat {
    pub theme: Theme,
    pub label: String,
    /// 最近 30 天。
    pub recent: Counts,
    /// 全部历史。
    pub all: Counts,
    /// 独立选对过的不同局面数。
    pub positions: u32,
    /// 隔天以后再做时独立选对的次数。
    pub spaced: u32,
    /// 同一局面当天重复作答时独立选对的次数。
    pub repeated: u32,
    /// 这个主题的复习卡片张数。
    pub cards: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhaseStat {
    pub phase: Phase,
    pub label: String,
    pub recent: Counts,
}

/// 最近反复出错的地方。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Weakness {
    pub theme: Theme,
    pub text: String,
    /// 可以打开的例子：最近没有独立选对的卡片，新的在前，最多 3 张。
    pub examples: Vec<String>,
}

/// 今天最值得练的一组。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Recommendation {
    pub theme: Theme,
    pub cards: u32,
    pub text: String,
}

/// 是真的掌握了还是只记住了答案：每个主题的依据。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MasteryNote {
    pub theme: Theme,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LearningProfile {
    pub generated_at: i64,
    /// 全部作答（不含比较中与无法评价）。
    pub answered: u32,
    /// 作答足够多，可以给出下面的判断。
    pub enough: bool,
    /// 作答还不多时的说明。
    pub note: Option<String>,
    pub weaknesses: Vec<Weakness>,
    pub recommendation: Option<Recommendation>,
    pub mastery: Vec<MasteryNote>,
    pub themes: Vec<ThemeStat>,
    pub phases: Vec<PhaseStat>,
}

/// 同一局面的作答（复习按卡片，先想后讲按会话中的这一着）。
fn position_key(e: &LearningEvent) -> String { e.card.clone().unwrap_or_else(|| format!("{}/{:010}", e.study_id, e.node_id)) }

/// 由学习事件统计画像。`cards` 是每个主题的复习卡片张数（推荐练习时要有卡片可练）。
pub fn build(events: &[LearningEvent], cards: &HashMap<Theme, usize>, now: i64) -> LearningProfile {
    let mut sorted: Vec<&LearningEvent> = events.iter().collect();
    sorted.sort_by_key(|e| e.at);
    let recent_from = now - RECENT_DAYS * DAY_MS;

    let mut themes: HashMap<Theme, ThemeStat> = Theme::ALL
        .iter()
        .map(|t| {
            let stat = ThemeStat {
                theme: *t,
                label: t.label().to_string(),
                recent: Counts::default(),
                all: Counts::default(),
                positions: 0,
                spaced: 0,
                repeated: 0,
                cards: cards.get(t).copied().unwrap_or(0) as u32,
            };
            (*t, stat)
        })
        .collect();
    let mut phases: HashMap<Phase, Counts> = HashMap::new();
    let mut total = Counts::default();
    let mut last_at: HashMap<String, i64> = HashMap::new();
    let mut solved: HashMap<Theme, Vec<String>> = HashMap::new();
    let mut examples: HashMap<Theme, Vec<String>> = HashMap::new();

    for e in &sorted {
        let key = position_key(e);
        let previous = last_at.insert(key.clone(), e.at);
        let recent = e.at >= recent_from;
        total.add(e);
        if recent && let Some(phase) = Phase::of(&e.fen) {
            phases.entry(phase).or_default().add(e);
        }
        for theme in &e.themes {
            let Some(stat) = themes.get_mut(theme) else { continue };
            stat.all.add(e);
            if recent {
                stat.recent.add(e);
            }
            if e.outcome == EventOutcome::Independent {
                let seen = solved.entry(*theme).or_default();
                if !seen.contains(&key) {
                    seen.push(key.clone());
                }
                match previous {
                    Some(p) if e.at - p >= SPACED_MS => stat.spaced += 1,
                    Some(_) => stat.repeated += 1,
                    None => {}
                }
            }
            let missed = matches!(e.outcome, EventOutcome::Missed | EventOutcome::Revealed | EventOutcome::Assisted);
            if recent
                && missed
                && let Some(card) = &e.card
            {
                let list = examples.entry(*theme).or_default();
                list.retain(|c| c != card);
                list.insert(0, card.clone());
            }
        }
    }
    for (theme, keys) in &solved {
        if let Some(stat) = themes.get_mut(theme) {
            stat.positions = keys.len() as u32;
        }
    }

    let mut theme_list: Vec<ThemeStat> = Theme::ALL.iter().filter_map(|t| themes.remove(t)).collect();
    let phase_list: Vec<PhaseStat> = Phase::ALL
        .iter()
        .map(|p| PhaseStat { phase: *p, label: p.label().to_string(), recent: phases.get(p).copied().unwrap_or_default() })
        .collect();
    let enough = total.answered >= MIN_ANSWERED;
    let note = (!enough).then(|| {
        format!("作答还不多（{} 次），先多练一些；有 {MIN_ANSWERED} 次以上作答后，这里会说明反复出错的地方和值得练的一组。", total.answered)
    });

    // 最近反复出错的地方：最近作答足够多、一半以上没有独立选对的主题，按没有独立选对的次数排列
    let mut weak: Vec<&ThemeStat> = theme_list
        .iter()
        .filter(|s| s.recent.answered >= MIN_THEME && s.recent.not_independent() * 2 >= s.recent.answered && s.recent.not_independent() >= 3)
        .collect();
    weak.sort_by(|a, b| b.recent.not_independent().cmp(&a.recent.not_independent()).then(b.recent.answered.cmp(&a.recent.answered)));
    let weaknesses: Vec<Weakness> = if enough {
        weak.iter()
            .take(3)
            .map(|s| Weakness {
                theme: s.theme,
                text: weakness_text(s),
                examples: examples.get(&s.theme).map(|l| l.iter().take(3).cloned().collect()).unwrap_or_default(),
            })
            .collect()
    } else {
        Vec::new()
    };

    // 今天最值得练的一组：最需要练、又有卡片可练的主题
    let recommendation = weak.iter().find(|s| s.cards as usize >= MIN_CARDS).filter(|_| enough).map(|s| Recommendation {
        theme: s.theme,
        cards: s.cards,
        text: format!(
            "建议先练“{}”这一组（{} 张卡片）：最近 {} 次作答中 {} 次没有独立选对。",
            s.label,
            s.cards,
            s.recent.answered,
            s.recent.not_independent()
        ),
    });

    // 掌握还是记住了答案：独立选对过两次以上的主题，按独立选对的次数排列
    theme_list.sort_by_key(|s| std::cmp::Reverse(s.all.independent));
    let mastery: Vec<MasteryNote> = if enough {
        theme_list.iter().filter(|s| s.all.independent >= 2).take(3).map(|s| MasteryNote { theme: s.theme, text: mastery_text(s) }).collect()
    } else {
        Vec::new()
    };
    theme_list.sort_by_key(|s| Theme::ALL.iter().position(|t| *t == s.theme));

    LearningProfile {
        generated_at: now,
        answered: total.answered,
        enough,
        note,
        weaknesses,
        recommendation,
        mastery,
        themes: theme_list,
        phases: phase_list,
    }
}

fn weakness_text(s: &ThemeStat) -> String {
    let c = &s.recent;
    let mut parts = Vec::new();
    if c.missed > 0 {
        parts.push(format!("没有选对 {} 次", c.missed));
    }
    if c.revealed > 0 {
        parts.push(format!("直接看答案 {} 次", c.revealed));
    }
    if c.assisted > 0 {
        parts.push(format!("看了提示后选对 {} 次", c.assisted));
    }
    let mut text = format!(
        "最近 {RECENT_DAYS} 天涉及“{}”的 {} 次作答中，{} 次没有独立选对（{}）",
        s.label,
        c.answered,
        c.not_independent(),
        parts.join("，")
    );
    if c.confident_misses > 0 {
        text.push_str(&format!("；其中 {} 次很有把握却没有选对", c.confident_misses));
    }
    text.push('。');
    text
}

fn mastery_text(s: &ThemeStat) -> String {
    let lead = format!("“{}”：独立选对 {} 次（{} 个不同局面）", s.label, s.all.independent, s.positions);
    if s.spaced >= 2 && s.positions >= 2 {
        format!("{lead}，其中 {} 次是隔天以后再做时选对，依据比较充分。", s.spaced)
    } else if s.repeated > s.spaced {
        format!("{lead}，其中 {} 次是同一局面当天重复作答；隔天以后再做时选对 {} 次，还需要隔天再检验。", s.repeated, s.spaced)
    } else if s.spaced == 0 {
        format!("{lead}，还没有隔天以后再做的记录。")
    } else {
        format!("{lead}，其中 {} 次是隔天以后再做时选对。", s.spaced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::PracticeGoal;
    use crate::learning::EventSource;

    const NOW: i64 = 100 * DAY_MS;
    /// 炮二平五之后（开局）。
    const OPENING: &str = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR b";
    /// 车马炮各剩一个（残局）。
    const ENDGAME: &str = "3k5/9/4c4/9/9/9/9/4N4/9/R3K4 w";

    fn event(card: &str, at: i64, outcome: EventOutcome, themes: &[Theme]) -> LearningEvent {
        LearningEvent {
            id: format!("{card}/{at}"),
            at,
            source: EventSource::Review,
            card: Some(card.into()),
            study_id: "s".into(),
            node_id: 1,
            fen: OPENING.into(),
            goal: PracticeGoal::Recall,
            outcome,
            themes: themes.to_vec(),
            issues: vec![],
            hints: 0,
            intent: None,
            elapsed_ms: None,
            confidence: None,
            stability: None,
            practice: None,
        }
    }

    fn cards(n: usize) -> HashMap<Theme, usize> { Theme::ALL.iter().map(|t| (*t, n)).collect() }

    #[test]
    fn few_answers_are_said_plainly_without_conclusions() {
        let events = [
            event("a", NOW - 10, EventOutcome::Missed, &[Theme::Threatened]),
            event("a", NOW - 5, EventOutcome::Missed, &[Theme::Threatened]),
            event("b", NOW - 4, EventOutcome::Pending, &[Theme::Threatened]),
        ];
        let p = build(&events, &cards(3), NOW);
        assert_eq!((p.answered, p.enough), (2, false), "比较中不算作答");
        assert_eq!(p.note.as_deref(), Some("作答还不多（2 次），先多练一些；有 5 次以上作答后，这里会说明反复出错的地方和值得练的一组。"));
        assert!(p.weaknesses.is_empty() && p.recommendation.is_none() && p.mastery.is_empty());
    }

    #[test]
    fn repeated_mistakes_in_one_theme_give_a_weakness_with_examples_and_a_recommendation() {
        let mut sure_miss = event("c", NOW - 3 * DAY_MS, EventOutcome::Missed, &[Theme::Threatened, Theme::Protection]);
        sure_miss.confidence = Some(Confidence::Sure);
        let events = vec![
            event("a", NOW - 9 * DAY_MS, EventOutcome::Missed, &[Theme::Threatened]),
            event("b", NOW - 8 * DAY_MS, EventOutcome::Revealed, &[Theme::Threatened]),
            sure_miss,
            event("d", NOW - 2 * DAY_MS, EventOutcome::Assisted, &[Theme::Threatened]),
            event("e", NOW - DAY_MS, EventOutcome::Independent, &[Theme::Threatened]),
            event("f", NOW - DAY_MS, EventOutcome::Independent, &[Theme::Lines]),
            // 40 天前的不算“最近”
            event("g", NOW - 40 * DAY_MS, EventOutcome::Missed, &[Theme::Threatened]),
        ];
        let p = build(&events, &cards(3), NOW);
        assert!(p.enough);
        assert_eq!(p.weaknesses.len(), 1);
        let w = &p.weaknesses[0];
        assert_eq!(w.theme, Theme::Threatened);
        assert_eq!(
            w.text,
            "最近 30 天涉及“受攻击棋子的处理”的 5 次作答中，4 次没有独立选对（没有选对 2 次，直接看答案 1 次，看了提示后选对 1 次）；其中 1 次很有把握却没有选对。"
        );
        assert_eq!(w.examples, ["d", "c", "b"], "新的在前，最多 3 张");
        let r = p.recommendation.unwrap();
        assert_eq!((r.theme, r.cards), (Theme::Threatened, 3));
        assert_eq!(r.text, "建议先练“受攻击棋子的处理”这一组（3 张卡片）：最近 5 次作答中 4 次没有独立选对。");
        // 卡片不够两张时不推荐
        let few = build(&events, &cards(1), NOW);
        assert!(few.recommendation.is_none() && !few.weaknesses.is_empty());
        // 阶段分开统计
        assert_eq!(p.phases.iter().find(|s| s.phase == Phase::Opening).unwrap().recent.answered, 6);
    }

    #[test]
    fn mastery_tells_spaced_success_in_different_positions_from_same_day_repeats() {
        let mut events = Vec::new();
        // 保护关系：两个局面都在隔天以后再做时独立选对
        for card in ["p1", "p2"] {
            events.push(event(card, NOW - 5 * DAY_MS, EventOutcome::Missed, &[Theme::Protection]));
            events.push(event(card, NOW - 2 * DAY_MS, EventOutcome::Independent, &[Theme::Protection]));
        }
        // 炮架与线路：同一张卡片当天重复答对
        events.push(event("l1", NOW - 3_600_000 * 3, EventOutcome::Missed, &[Theme::Lines]));
        for i in 0..3 {
            events.push(event("l1", NOW - 3_600_000 * (2 - i), EventOutcome::Independent, &[Theme::Lines]));
        }
        let p = build(&events, &cards(3), NOW);
        let text = |t: Theme| p.mastery.iter().find(|m| m.theme == t).unwrap().text.clone();
        assert_eq!(text(Theme::Protection), "“保护关系变化”：独立选对 2 次（2 个不同局面），其中 2 次是隔天以后再做时选对，依据比较充分。");
        assert_eq!(
            text(Theme::Lines),
            "“炮架与线路变化”：独立选对 3 次（1 个不同局面），其中 3 次是同一局面当天重复作答；隔天以后再做时选对 0 次，还需要隔天再检验。"
        );
    }

    #[test]
    fn phases_are_split_by_material() {
        assert_eq!(Phase::of(OPENING), Some(Phase::Opening));
        assert_eq!(Phase::of(ENDGAME), Some(Phase::Endgame));
        assert_eq!(Phase::of("坏的局面"), None);
    }
}
