//! 检查模型输出。JSON 合法不代表棋理正确；这里拦截程序能够判断的问题：
//!
//! - 结构：字段、段落数量、篇幅；
//! - 引用：编号必须来自本次证据包，每段都要引用与段落类型相符的结论；
//! - 硬结论：新增的着法、胜率、未经确认的杀棋、分析不完整时的优劣评价、
//!   与评价等级不符的失误说法、绝对化措辞，以及把讲解写成棋手真实想法。
//!
//! 能力边界：自由文字的语义正确性无法由程序完全保证。“数字在输入里出现过”不能说明它在句子里用对了
//! （正负号、属于哪个候选、是深度还是分差），因此文字中不允许出现数字：评分、分差、深度、将杀步数
//! 都由程序在每段所引用的依据中显示。将杀与优劣评价的说法必须在同一段引用支持它的结论，
//! 并检查该结论的可信程度；其余措辞只能做到不与程序给出的结论明显冲突。

use std::collections::HashSet;

use xq_core::explain::Certainty;
use xq_core::explain::VerdictTone;

use super::CoachExplanation;
use super::FollowupAction;
use super::SectionKind;
use super::evidence::EvidenceBundle;
use super::prompt::length_rule;
use crate::config::Detail;

const PIECES: &str = "车马炮相象仕士帅将兵卒俥傌砲帥";
const PREFIXES: &str = "前中后一二三四五";
const DIGITS: &str = "一二三四五六七八九123456789１２３４５６７８９";

/// 除非程序给出的结论里本来就有，否则不能出现的说法。
const ABSOLUTE: [&str; 15] =
    ["胜率", "%", "％", "百分之", "必胜", "必败", "必然", "无论怎么走", "一定能", "最佳", "最优", "最强", "唯一", "稳赢", "必杀"];
/// 确定的杀棋说法：同一段必须引用已经确认的将杀结论。
const MATE_WORDS: [&str; 7] = ["将死", "绝杀", "杀棋", "步杀", "被杀", "杀局", "成杀"];
/// 能支持杀棋说法的结论类别。
const MATE_KINDS: [&str; 3] = ["mate", "mate_line", "mated_line"];
const CN_NUMERALS: &str = "零一二三四五六七八九十百千两";
const BAD_WORDS: [&str; 8] = ["失误", "败着", "劣着", "错着", "漏着", "恶手", "缓着", "疑问手"];
const COMPARE_WORDS: [&str; 5] = ["首选", "更好", "更差", "不如", "略逊"];
const INTENT_WORDS: [&str; 5] = ["本意", "意图是", "心里想", "他想", "她想"];

const SUMMARY_MAX: usize = 80;
const LESSON_MAX: usize = 120;

/// 文字中的中文记谱（四字形式，如“炮二平五”“马8进7”“前车进一”）及其位置。
fn notations(text: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= chars.len() {
        let w = &chars[i..i + 4];
        let head_ok = PIECES.contains(w[0]) || PREFIXES.contains(w[0]);
        if head_ok && (PIECES.contains(w[1]) || DIGITS.contains(w[1])) && "进退平".contains(w[2]) && DIGITS.contains(w[3])
        {
            out.push((i, w.iter().collect()));
            i += 4;
        } else {
            i += 1;
        }
    }
    out
}

/// 用汉字写出的分数与将杀步数，如“三十分”“三步杀”。“十分”（很）不算。
fn spelled_numbers(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !CN_NUMERALS.contains(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && CN_NUMERALS.contains(chars[i]) {
            i += 1;
        }
        let run: String = chars[start..i].iter().collect();
        let next = chars.get(i).copied();
        let score = next == Some('分') && run != "十";
        // “三步杀”“三步之内就会杀棋”：步字后不远处出现“杀”
        let mate = next == Some('步') && chars.iter().skip(i + 1).take(6).any(|c| *c == '杀');
        if score || mate {
            out.push(chars[start..(i + 1).min(chars.len())].iter().collect());
        }
    }
    out
}

/// 文字中的阿拉伯数字（记谱中的数字除外）。
fn numbers(text: &str) -> Vec<String> {
    let mut chars: Vec<char> = text.chars().collect();
    for (i, _) in notations(text) {
        for c in chars.iter_mut().skip(i).take(4) {
            *c = ' ';
        }
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in chars {
        let digit = match c {
            '0'..='9' => Some(c),
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32),
            _ => None,
        };
        match digit {
            Some(d) => cur.push(d),
            None if !cur.is_empty() => out.push(std::mem::take(&mut cur)),
            None => {}
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 证据包中可以出现在讲解里的着法来源（实际着法、结论、候选与历史）。
fn source_text(bundle: &EvidenceBundle) -> String {
    let p = &bundle.pack;
    let mut parts: Vec<String> = vec![p.move_notation.clone()];
    parts.extend(p.claims.iter().map(|c| c.statement.clone()));
    for c in &p.candidates {
        parts.push(format!("{} {} {}", c.notation, c.line.join(" "), c.effect));
    }
    parts.extend(p.warnings.iter().cloned());
    parts.extend(p.history.moves.iter().cloned());
    parts.join("\n")
}

fn strip_fence(raw: &str) -> &str {
    let t = raw.trim();
    let Some(rest) = t.strip_prefix("```") else { return t };
    let rest = rest.split_once('\n').map(|(_, body)| body).unwrap_or("");
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

/// 每种段落至少要引用一条相应来源的结论。
fn groups_for(kind: SectionKind) -> &'static [&'static str] {
    match kind {
        SectionKind::Purpose => &["need", "basis", "verdict"],
        SectionKind::Reason => &["basis", "need"],
        SectionKind::Comparison => &["comparison", "verdict", "candidate"],
        SectionKind::Reply => &["reply", "candidate"],
        SectionKind::Caution => &["need", "basis", "comparison", "reply", "verdict", "candidate", "lesson"],
    }
}

fn kind_label(kind: SectionKind) -> &'static str {
    match kind {
        SectionKind::Purpose => "为什么考虑这步",
        SectionKind::Reason => "为什么这样走",
        SectionKind::Comparison => "换一种走法",
        SectionKind::Reply => "下一步注意",
        SectionKind::Caution => "风险提示",
    }
}

/// 检查并整理模型输出；不通过时返回全部问题，便于让模型一次修正。
pub fn validate(raw: &str, bundle: &EvidenceBundle, detail: Detail) -> Result<CoachExplanation, Vec<String>> {
    let mut out: CoachExplanation =
        serde_json::from_str(strip_fence(raw)).map_err(|e| vec![format!("输出不是符合结构的 JSON（{e}）")])?;
    let mut errors = Vec::new();
    let pack = &bundle.pack;
    let rule = length_rule(detail);

    // 结构与篇幅
    let n = out.sections.len();
    if n < rule.sections.0 || n > rule.sections.1 {
        errors.push(format!("段落数应为 {}～{} 段，实际 {n} 段", rule.sections.0, rule.sections.1));
    }
    let len = |t: &str| t.chars().count();
    if out.summary.text.trim().is_empty() {
        errors.push("一句话结论为空".into());
    } else if len(&out.summary.text) > SUMMARY_MAX {
        errors.push(format!("一句话结论超过 {SUMMARY_MAX} 字"));
    }
    if len(&out.lesson.text) > LESSON_MAX {
        errors.push(format!("学习要点超过 {LESSON_MAX} 字"));
    }
    let total = len(&out.summary.text) + len(&out.lesson.text) + out.sections.iter().map(|s| len(&s.text)).sum::<usize>();
    if total > rule.max_chars {
        errors.push(format!("全文 {total} 字，超过 {} 字的篇幅", rule.max_chars));
    }

    // 引用
    let mut check_refs = |label: &str, ids: &[String], required: bool| {
        if required && ids.is_empty() {
            errors.push(format!("{label}没有引用任何结论编号"));
        }
        for id in ids {
            if bundle.claim(id).is_none() {
                errors.push(format!("{label}引用了不存在的结论编号 {id}"));
            }
        }
    };
    check_refs("一句话结论", &out.summary.claim_ids, true);
    for (i, s) in out.sections.iter().enumerate() {
        check_refs(&format!("第 {} 段", i + 1), &s.claim_ids, true);
    }
    check_refs("学习要点", &out.lesson.claim_ids, false);
    for (i, s) in out.sections.iter().enumerate() {
        let groups = groups_for(s.kind);
        let fits = s.claim_ids.iter().filter_map(|id| bundle.claim(id)).any(|c| groups.contains(&c.group.as_str()));
        if !s.claim_ids.is_empty() && !fits {
            errors.push(format!("第 {} 段（{}）没有引用与之相符的依据", i + 1, kind_label(s.kind)));
        }
    }

    // 后续操作：由后台对应到可回放的证据，没有对应证据的不显示；按钮文字也由后台给出
    let mut actions = HashSet::new();
    out.followups.retain(|f| actions.insert(f.action));
    out.followups.truncate(3);
    for f in &mut out.followups {
        f.evidence_id = followup_evidence(bundle, f.action);
        f.label = followup_label(f.action).to_string();
    }
    out.followups.retain(|f| f.evidence_id.is_some());

    // 硬结论
    let source = source_text(bundle);
    let known_moves: HashSet<String> = notations(&source).into_iter().map(|(_, m)| m).collect();
    let tone = pack.verdict.as_ref().map(|v| v.tone);
    let complete = pack.analysis.complete;
    let claimed = |w: &str| pack.claims.iter().any(|c| c.statement.contains(w));
    // 每段文字及其引用的结论：将杀与优劣评价必须由同一段的引用支持
    let mut texts: Vec<(&str, &[String])> = vec![(&out.summary.text, &out.summary.claim_ids), (&out.lesson.text, &out.lesson.claim_ids)];
    texts.extend(out.sections.iter().map(|s| (s.text.as_str(), s.claim_ids.as_slice())));
    let mut seen = HashSet::new();
    let mut flag = |msg: String| {
        if seen.insert(msg.clone()) {
            errors.push(msg);
        }
    };
    for (text, ids) in texts {
        let cited: Vec<_> = ids.iter().filter_map(|id| bundle.claim(id)).collect();
        let cites = |groups: &[&str]| cited.iter().any(|c| groups.contains(&c.group.as_str()) || groups.contains(&c.kind.as_str()));
        for (_, m) in notations(text) {
            if !known_moves.contains(&m) {
                flag(format!("出现了证据中没有的着法“{m}”"));
            }
        }
        if let Some(num) = numbers(text).into_iter().next() {
            flag(format!("文字中不要写数字（出现“{num}”）：评分、分差、深度、将杀步数由程序在依据中显示"));
        }
        if let Some(num) = spelled_numbers(text).into_iter().next() {
            flag(format!("文字中不要写分数或将杀步数（出现“{num}”），这些由程序在依据中显示"));
        }
        for w in ABSOLUTE {
            if text.contains(w) && !claimed(w) {
                flag(format!("使用了证据不支持的说法“{w}”"));
            }
        }
        if let Some(w) = MATE_WORDS.iter().find(|w| text.contains(**w)) {
            // 规则确认的将死，或完整分析中的精确将杀分；“尚未完成的搜索”里的将杀不能写成确定结论
            let confirmed = cited.iter().any(|c| MATE_KINDS.contains(&c.kind.as_str()) && c.certainty != Certainty::Tentative);
            if !confirmed {
                flag(format!("说“{w}”时，同一段必须引用已经确认的将杀结论"));
            }
        }
        if !complete {
            if let Some(w) = BAD_WORDS.iter().chain(COMPARE_WORDS.iter()).find(|w| text.contains(**w)) {
                flag(format!("分析尚未完成，不能评价优劣（出现“{w}”）"));
            }
        } else {
            let bad_ok = matches!(tone, Some(VerdictTone::Inaccuracy | VerdictTone::Mistake | VerdictTone::Blunder));
            if let Some(w) = BAD_WORDS.iter().find(|w| text.contains(**w)) {
                if !bad_ok {
                    flag(format!("程序的评价不是失误，不能说“{w}”"));
                } else if !cites(&["verdict", "comparison"]) {
                    flag(format!("说“{w}”时，同一段必须引用程序的评价或候选比较"));
                }
            }
            if text.contains("严重") && tone != Some(VerdictTone::Blunder) && !claimed("严重") {
                flag("程序的评价不是严重失误，不能说“严重”".to_string());
            }
            if let Some(w) = COMPARE_WORDS.iter().find(|w| text.contains(**w))
                && !cites(&["verdict", "comparison", "candidate"])
            {
                flag(format!("说“{w}”时，同一段必须引用程序的评价、候选比较或候选"));
            }
        }
        if let Some(w) = INTENT_WORDS.iter().find(|w| text.contains(**w)) {
            flag(format!("不要把讲解写成棋手的真实想法（出现“{w}”）"));
        }
    }

    if errors.is_empty() { Ok(out) } else { Err(errors) }
}

fn followup_label(action: FollowupAction) -> &'static str {
    match action {
        FollowupAction::ShowReply => "演示对方的应对",
        FollowupAction::ShowBest => "演示首选的变化",
        FollowupAction::CompareAlternative => "看另一个候选",
    }
}

/// 后续操作对应的回放证据：只选择带有合法变化的证据。
fn followup_evidence(bundle: &EvidenceBundle, action: FollowupAction) -> Option<String> {
    let replayable = |id: &str| bundle.evidence.iter().any(|e| e.id == id && e.line.is_some());
    let by_topic = |topic: &str| bundle.evidence.iter().find(|e| e.topic == topic && e.line.is_some()).map(|e| e.id.clone());
    let candidate = |pred: &dyn Fn(&super::evidence::PackCandidate) -> bool| {
        bundle.pack.candidates.iter().position(pred).map(|i| format!("k{}", i + 1)).filter(|id| replayable(id))
    };
    match action {
        FollowupAction::ShowReply => by_topic("reply"),
        // 分析不完整时没有确定的首选
        FollowupAction::ShowBest if !bundle.pack.analysis.complete => None,
        FollowupAction::ShowBest => by_topic("best").or_else(|| candidate(&|c| c.rank == Some(1))),
        FollowupAction::CompareAlternative => by_topic("second").or_else(|| candidate(&|c| !c.is_actual)),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::llm::test_support::*;

    fn output(summary: &str, sections: serde_json::Value) -> String {
        json!({
            "summary": { "text": summary, "claim_ids": ["c1"] },
            "sections": sections,
            "lesson": { "text": "出子时检查新增的保护关系。", "claim_ids": [] },
            "followups": [{ "action": "show_reply", "label": "演示对方的应对" }, { "action": "show_best", "label": "看首选" }],
        })
        .to_string()
    }

    fn claim_of(bundle: &EvidenceBundle, group: &str) -> String {
        bundle.pack.claims.iter().find(|c| c.group == group).unwrap().id.clone()
    }

    #[test]
    fn accepted_output_maps_followups_to_evidence() {
        let b = detailed_bundle(&[("h0g2", 30, &["h0g2", "h9g7"]), ("b0c2", 20, &["b0c2"])], true);
        let basis = claim_of(&b, "basis");
        let cand = claim_of(&b, "candidate");
        let raw = format!(
            "```json\n{}\n```",
            output(
                "马二进三出动右马，并保护中兵。",
                json!([
                    { "kind": "purpose", "text": "当前没有直接威胁，出动车马十分重要。", "claim_ids": [basis] },
                    { "kind": "comparison", "text": "马二进三在本次搜索中是首选。", "claim_ids": [cand] },
                ]),
            )
        );
        let out = validate(&raw, &b, Detail::Normal).unwrap_or_else(|e| panic!("{e:?}"));
        let reply = out.followups.iter().find(|f| f.action == FollowupAction::ShowReply).unwrap();
        assert!(reply.evidence_id.as_ref().is_some_and(|id| b.evidence.iter().any(|e| &e.id == id && e.line.is_some())));
        // 按钮文字由后台给出
        let best = out.followups.iter().find(|f| f.action == FollowupAction::ShowBest).unwrap();
        assert_eq!(best.label, "演示首选的变化");
    }

    #[test]
    fn invented_content_is_rejected() {
        let b = detailed_bundle(&[("h0g2", 30, &["h0g2", "h9g7"]), ("b0c2", 20, &["b0c2"])], true);
        let basis = claim_of(&b, "basis");
        let cand = claim_of(&b, "candidate");
        let reply = claim_of(&b, "reply");
        let cases = [
            ("引用了不存在", json!([{ "kind": "purpose", "text": "出动右马。", "claim_ids": ["c99"] }])),
            ("没有引用任何结论", json!([{ "kind": "purpose", "text": "出动右马。", "claim_ids": [] }])),
            ("没有引用与之相符", json!([{ "kind": "comparison", "text": "出动右马。", "claim_ids": [basis] }])),
            ("没有的着法“车一进一”", json!([{ "kind": "reply", "text": "对方可以车一进一。", "claim_ids": [reply] }])),
            // 评审中的反例：评分正负号反转、把一个候选的评分写给另一个、把深度写成分差
            ("不要写数字（出现“30”）", json!([{ "kind": "comparison", "text": "马二进三的评分为 -30。", "claim_ids": [cand] }])),
            ("不要写数字（出现“30”）", json!([{ "kind": "comparison", "text": "马八进七的评分是 +30。", "claim_ids": [cand] }])),
            ("不要写数字（出现“10”）", json!([{ "kind": "comparison", "text": "比第二候选高约 10 分。", "claim_ids": [cand] }])),
            ("不要写分数或将杀步数（出现“三十分”）", json!([{ "kind": "comparison", "text": "大约领先三十分。", "claim_ids": [cand] }])),
            ("不要写分数或将杀步数（出现“三步”）", json!([{ "kind": "reply", "text": "对方三步之内就会杀棋。", "claim_ids": [reply] }])),
            ("“胜率”", json!([{ "kind": "comparison", "text": "红方胜率很高。", "claim_ids": [cand] }])),
            ("已经确认的将杀结论", json!([{ "kind": "reply", "text": "对方很快会被杀。", "claim_ids": [reply] }])),
            ("不是失误", json!([{ "kind": "comparison", "text": "这步是失误。", "claim_ids": [cand] }])),
            ("“最佳”", json!([{ "kind": "comparison", "text": "这是最佳选择。", "claim_ids": [cand] }])),
            ("真实想法", json!([{ "kind": "purpose", "text": "他想出动右马。", "claim_ids": [basis] }])),
        ];
        for (expect, sections) in cases {
            let errors = validate(&output("出动右马。", sections), &b, Detail::Normal).unwrap_err();
            assert!(errors.iter().any(|e| e.contains(expect)), "{expect}: {errors:?}");
        }
        assert!(validate("{\"summary\":", &b, Detail::Normal).unwrap_err()[0].contains("JSON"));
        let long = "很长".repeat(200);
        let errors =
            validate(&output("出动右马。", json!([{ "kind": "purpose", "text": long, "claim_ids": [basis] }])), &b, Detail::Brief)
                .unwrap_err();
        assert!(errors.iter().any(|e| e.contains("篇幅")), "{errors:?}");
    }

    #[test]
    fn incomplete_analysis_forbids_verdict_words() {
        let b = detailed_bundle(&[("h2e2", 400, &["h2e2"]), ("h0g2", -100, &["h0g2"])], false);
        let cand = claim_of(&b, "candidate");
        for word in ["首选", "失误", "不如"] {
            let sections = json!([{ "kind": "comparison", "text": format!("这步{word}炮二平五。"), "claim_ids": [cand] }]);
            let errors = validate(&output("出动右马。", sections), &b, Detail::Normal).unwrap_err();
            assert!(errors.iter().any(|e| e.contains("分析尚未完成")), "{word}: {errors:?}");
        }
        // 没有确定的首选可演示
        let ok = validate(
            &output("出动右马。", json!([{ "kind": "comparison", "text": "候选比较还没有完成。", "claim_ids": [cand] }])),
            &b,
            Detail::Normal,
        )
        .unwrap();
        assert!(ok.followups.iter().all(|f| f.action != FollowupAction::ShowBest));
    }

    #[test]
    fn mate_wording_needs_a_confirmed_cited_mate_claim() {
        let text = "这步之后可以形成杀棋。";
        // 尚未完成的搜索中的将杀：可信程度为 tentative，不能写成确定的杀棋
        let tentative = mate_bundle(false);
        let mate = tentative.pack.claims.iter().find(|c| c.kind == "mate_line").unwrap();
        assert_eq!(mate.certainty, Certainty::Tentative);
        let sections = json!([{ "kind": "reply", "text": text, "claim_ids": [mate.id] }]);
        let errors = validate(&output("出动右马。", sections), &tentative, Detail::Normal).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("已经确认的将杀结论")), "{errors:?}");
        // 完整分析中的精确将杀分：引用它时可以说，不引用时不行
        let confirmed = mate_bundle(true);
        let mate = confirmed.pack.claims.iter().find(|c| c.kind == "mate_line").unwrap();
        assert_eq!(mate.certainty, Certainty::Supported);
        let sections = json!([{ "kind": "reply", "text": text, "claim_ids": [mate.id] }]);
        validate(&output("出动右马。", sections), &confirmed, Detail::Normal).unwrap_or_else(|e| panic!("{e:?}"));
        let basis = claim_of(&confirmed, "basis");
        let sections = json!([{ "kind": "purpose", "text": text, "claim_ids": [basis] }]);
        let errors = validate(&output("出动右马。", sections), &confirmed, Detail::Normal).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("已经确认的将杀结论")), "{errors:?}");
    }

    #[test]
    fn mistake_words_follow_program_verdict() {
        // 实际着法比首选低 380 分：程序评价为严重失误，引用评价或比较时可以这样说
        let b = blunder_bundle();
        assert_eq!(b.pack.verdict.as_ref().unwrap().tone, VerdictTone::Blunder);
        let cmp = claim_of(&b, "comparison");
        let text = "这步是严重失误，比首选差得多。";
        let sections = json!([{ "kind": "comparison", "text": text, "claim_ids": [cmp] }]);
        validate(&output("炮五进四吃卒，但会丢炮。", sections), &b, Detail::Normal).unwrap_or_else(|e| panic!("{e:?}"));
        let basis = claim_of(&b, "basis");
        let sections = json!([{ "kind": "caution", "text": text, "claim_ids": [basis] }]);
        let errors = validate(&output("炮五进四吃卒，但会丢炮。", sections), &b, Detail::Normal).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("同一段必须引用程序的评价")), "{errors:?}");
    }

    #[test]
    fn illegal_candidate_lines_are_not_replayed() {
        // 第二步重复走 h0g2 不合法：核心讲解已移除这条变化，证据包与后续操作也不能再回放它
        let b = detailed_bundle(&[("h0g2", 30, &["h0g2", "h0g2"]), ("b0c2", 20, &["b0c2"])], true);
        assert!(b.evidence.iter().find(|e| e.id == "k1").unwrap().line.is_none());
        assert!(b.pack.candidates[0].line.is_empty());
        let basis = claim_of(&b, "basis");
        let sections = json!([{ "kind": "purpose", "text": "出动右马。", "claim_ids": [basis] }]);
        let out = validate(&output("出动右马。", sections), &b, Detail::Normal).unwrap();
        assert!(out.followups.iter().all(|f| f.action != FollowupAction::ShowBest), "{:?}", out.followups);
    }
}
