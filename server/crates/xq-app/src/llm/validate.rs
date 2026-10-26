//! 检查模型输出。JSON 合法不代表棋理正确；这里拦截程序能够判断的问题：
//!
//! - 结构：字段、段落数量、篇幅；
//! - 引用：编号必须来自本次证据包，每段都要引用与段落类型相符的结论；
//! - 硬结论：数字、排名、评价等级、优劣方向、杀棋与胜负一律不写在模型文字中；
//!   另外拦截新增的着法、胜率与绝对化措辞，以及把讲解写成棋手真实想法。
//!
//! 能力边界：自由文字的语义无法由程序完全验证。引用了“正确类别”的结论，并不能说明句子里
//! “谁是首选”“谁更好”“哪一方会被杀”与程序结论一致；“数字在输入里出现过”也不能说明正负号与归属。
//! 因此这些硬结论只由程序给出：模型通过 `claim_ids` 选择要讲的结论，程序在每段下方显示结论原文
//! （排名、评分、评价等级、杀棋方向都在其中），模型文字只解释作用、理由与需要注意的地方。

use std::collections::HashSet;


use super::CoachExplanation;
use super::FollowupAction;
use super::SectionKind;
use super::ask::AskBundle;
use super::ask::CoachAnswer;
use super::evidence::AllowedClaim;
use super::evidence::EvidenceBundle;
use super::prompt::length_rule;
use crate::config::Detail;

const PIECES: &str = "车马炮相象仕士帅将兵卒俥傌砲帥";
const PREFIXES: &str = "前中后一二三四五";
const DIGITS: &str = "一二三四五六七八九123456789１２３４５６７８９";

/// 除非程序给出的结论里本来就有，否则不能出现的说法。
const ABSOLUTE: [&str; 15] =
    ["胜率", "%", "％", "百分之", "必胜", "必败", "必然", "无论怎么走", "一定能", "最佳", "最优", "最强", "唯一", "稳赢", "必杀"];
/// 硬结论的说法：排名、评价等级、优劣方向、杀棋（任何带“杀”字的说法）与胜负。由程序在依据中给出，模型文字不写。
const HARD_WORDS: [&str; 46] = [
    // 排名
    "首选", "次选", "第一候选", "第二候选", "第三候选", "排在", "排名",
    // 评价等级
    "失误", "败着", "劣着", "错着", "漏着", "恶手", "缓着", "疑问手", "好棋", "妙手", "佳着",
    // 优劣方向
    "更好", "更差", "更强", "更弱", "更高", "更低", "不如", "略逊", "优于", "劣于", "强于", "弱于", "胜过", "好于", "差于",
    "领先", "落后", "占优", "上风", "下风",
    // 杀棋与胜负
    "杀", "将死", "困毙", "胜势", "败势", "获胜", "取胜", "输棋",
];
const CN_NUMERALS: &str = "零一二三四五六七八九十百千万两";
/// 这些语境中的汉字数字是评分、深度等数值。
const NUMBER_CONTEXT: [&str; 4] = ["评分", "深度", "分差", "分数"];
/// 紧接在这些字后面的“十分”表示数量（如“领先十分”“差了十分”），而不是“很”。
const AMOUNT_BEFORE: &str = "先后差低高了约为是到达";
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

/// 去掉记谱后的文字：记谱中的数字（如“马二进三”的“二”“三”）不参与数字检查。
fn without_notations(text: &str) -> Vec<char> {
    let mut chars: Vec<char> = text.chars().collect();
    for (i, _) in notations(text) {
        for c in chars.iter_mut().skip(i).take(4) {
            *c = ' ';
        }
    }
    chars
}

fn is_cjk(c: char) -> bool { ('\u{4e00}'..='\u{9fff}').contains(&c) }

/// 用汉字写出的数值，如“负三十”“深度为二十”“领先十分”“三步杀”。记谱中的数字与“十分重要”中的“十分”不算。
fn spelled_numbers(text: &str) -> Vec<String> {
    let chars = without_notations(text);
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
        let prev = start.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i).copied();
        let before: String = chars[start.saturating_sub(4)..start].iter().collect();
        // 带正负号，或“三十”“一百”这样的多位数
        let signed = matches!(prev, Some('正' | '负'));
        let compound = run.chars().count() >= 2 && run.chars().any(|c| "十百千万".contains(c));
        // “评分为三”“深度二十”
        let in_context = NUMBER_CONTEXT.iter().any(|w| before.contains(w));
        // “三十分”“领先十分”是分数；“十分重要”的“十分”是“很”
        let adverb = run == "十" && chars.get(i + 1).is_some_and(|c| is_cjk(*c)) && !prev.is_some_and(|p| AMOUNT_BEFORE.contains(p));
        let points = next == Some('分') && !adverb;
        // “三步杀”“三步之内就会杀棋”
        let mate = next == Some('步') && chars.iter().skip(i + 1).take(6).any(|c| *c == '杀');
        if signed || compound || in_context || points || mate {
            let from = if signed { start - 1 } else { start };
            let to = if points || mate { i + 1 } else { i };
            out.push(chars[from..to.min(chars.len())].iter().collect());
        }
    }
    out
}

/// 文字中的阿拉伯数字（记谱中的数字除外）。
fn numbers(text: &str) -> Vec<String> {
    let chars = without_notations(text);
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

    let mut texts: Vec<&str> = vec![&out.summary.text, &out.lesson.text];
    texts.extend(out.sections.iter().map(|s| s.text.as_str()));
    errors.extend(text_problems(&texts, &source_text(bundle), &pack.claims));

    if errors.is_empty() { Ok(out) } else { Err(errors) }
}

/// 模型文字中的硬结论与新增着法（AI 讲解与追问共用）。`source` 是证据中允许出现的着法来源；
/// 绝对化说法只有在程序给出的结论里本来就有时才允许。
fn text_problems(texts: &[&str], source: &str, claims: &[AllowedClaim]) -> Vec<String> {
    let known_moves: HashSet<String> = notations(source).into_iter().map(|(_, m)| m).collect();
    let claimed = |w: &str| claims.iter().any(|c| c.statement.contains(w));
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    let mut flag = |msg: String| {
        if seen.insert(msg.clone()) {
            errors.push(msg);
        }
    };
    for &text in texts {
        for (_, m) in notations(text) {
            if !known_moves.contains(&m) {
                flag(format!("出现了证据中没有的着法“{m}”"));
            }
        }
        if let Some(num) = numbers(text).into_iter().next() {
            flag(format!("文字中不要写数字（出现“{num}”）：评分、分差、深度、将杀步数由程序在依据中显示"));
        }
        if let Some(num) = spelled_numbers(text).into_iter().next() {
            flag(format!("文字中不要写数值（出现“{num}”）：评分、分差、深度、将杀步数由程序在依据中显示"));
        }
        // 去掉记谱再查，避免“将五平四”之类的记谱触发
        let plain: String = without_notations(text).into_iter().collect();
        if let Some(w) = HARD_WORDS.iter().find(|w| plain.contains(**w)) {
            flag(format!("“{w}”属于排名、评价、优劣或杀棋结论，由程序在依据中显示，文字中不要写"));
        }
        for w in ABSOLUTE {
            if text.contains(w) && !claimed(w) {
                flag(format!("使用了证据不支持的说法“{w}”"));
            }
        }
        if let Some(w) = INTENT_WORDS.iter().find(|w| text.contains(**w)) {
            flag(format!("不要把讲解写成棋手的真实想法（出现“{w}”）"));
        }
    }
    errors
}

const ANSWER_MAX: usize = 160;
const POINT_MAX: usize = 120;
const POINTS_MAX: usize = 3;
const CHECK_MAX: usize = 60;
const ANSWER_TOTAL: usize = 480;

/// 追问中可以出现在回答里的着法来源（两步着法、各自的变化、结论与此前的着法）。
fn ask_source(bundle: &AskBundle) -> String {
    let p = &bundle.pack;
    let mut parts: Vec<String> = vec![p.mine.notation.clone(), p.reference.notation.clone()];
    parts.extend(p.mine.line.iter().chain(p.reference.line.iter()).cloned());
    parts.extend(p.claims.iter().map(|c| c.statement.clone()));
    parts.extend(p.warnings.iter().cloned());
    parts.extend(p.history.iter().cloned());
    parts.join("\n")
}

/// 检查追问的回答：结构与篇幅、引用（回答必须引用与问题相符的结论）、硬结论与新增着法。
pub fn validate_answer(raw: &str, bundle: &AskBundle) -> Result<CoachAnswer, Vec<String>> {
    let out: CoachAnswer =
        serde_json::from_str(strip_fence(raw)).map_err(|e| vec![format!("输出不是符合结构的 JSON（{e}）")])?;
    let mut errors = Vec::new();
    let len = |t: &str| t.chars().count();
    if out.answer.text.trim().is_empty() {
        errors.push("回答为空".into());
    } else if len(&out.answer.text) > ANSWER_MAX {
        errors.push(format!("回答超过 {ANSWER_MAX} 字"));
    }
    if out.points.len() > POINTS_MAX {
        errors.push(format!("要点最多 {POINTS_MAX} 条，实际 {} 条", out.points.len()));
    }
    for (i, p) in out.points.iter().enumerate() {
        if p.text.trim().is_empty() || len(&p.text) > POINT_MAX {
            errors.push(format!("第 {} 条要点应为 1～{POINT_MAX} 字", i + 1));
        }
    }
    if out.check.trim().is_empty() || len(&out.check) > CHECK_MAX {
        errors.push(format!("留给学习者验证的问题应为 1～{CHECK_MAX} 字"));
    }
    let total = len(&out.answer.text) + len(&out.check) + out.points.iter().map(|p| len(&p.text)).sum::<usize>();
    if total > ANSWER_TOTAL {
        errors.push(format!("全文 {total} 字，超过 {ANSWER_TOTAL} 字的篇幅"));
    }

    let mut check_refs = |label: &str, ids: &[String]| {
        if ids.is_empty() {
            errors.push(format!("{label}没有引用任何结论编号"));
        }
        for id in ids {
            if bundle.claim(id).is_none() {
                errors.push(format!("{label}引用了不存在的结论编号 {id}"));
            }
        }
    };
    check_refs("回答", &out.answer.claim_ids);
    for (i, p) in out.points.iter().enumerate() {
        check_refs(&format!("第 {} 条要点", i + 1), &p.claim_ids);
    }
    let groups = bundle.question.kind.required_groups();
    let fits = out
        .answer
        .claim_ids
        .iter()
        .chain(out.points.iter().flat_map(|p| p.claim_ids.iter()))
        .filter_map(|id| bundle.claim(id))
        .any(|c| groups.contains(&c.group.as_str()));
    if !fits {
        errors.push(format!("回答没有引用与问题相符的依据（{}）", groups.join("、")));
    }

    let mut texts: Vec<&str> = vec![&out.answer.text, &out.check];
    texts.extend(out.points.iter().map(|p| p.text.as_str()));
    errors.extend(text_problems(&texts, &ask_source(bundle), &bundle.pack.claims));

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
                    // 记谱里的数字（“马二进三”的“三”）不能和后面的字拼成数值；“十分重要”的“十分”是“很”
                    { "kind": "purpose", "text": "马二进三分担中路防守，尽快出动车马十分重要。", "claim_ids": [basis] },
                    { "kind": "comparison", "text": "马八进七同样出动马，但没有保护中兵，两者的比较见依据。", "claim_ids": [cand] },
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

    fn rejects(b: &EvidenceBundle, kind: &str, text: &str, claim: &str, expect: &str) {
        let sections = json!([{ "kind": kind, "text": text, "claim_ids": [claim] }]);
        let errors = validate(&output("出动右马。", sections), b, Detail::Normal).unwrap_err();
        assert!(errors.iter().any(|e| e.contains(expect)), "{text} → 期望“{expect}”：{errors:?}");
    }

    #[test]
    fn invented_content_is_rejected() {
        let b = detailed_bundle(&[("h0g2", 30, &["h0g2", "h9g7"]), ("b0c2", 20, &["b0c2"])], true);
        let basis = claim_of(&b, "basis");
        let reply = claim_of(&b, "reply");
        let cases = [
            ("purpose", "出动右马。", "c99", "引用了不存在"),
            ("comparison", "出动右马。", basis.as_str(), "没有引用与之相符"),
            ("reply", "对方可以车一进一。", reply.as_str(), "没有的着法“车一进一”"),
            ("purpose", "他想出动右马。", basis.as_str(), "真实想法"),
        ];
        for (kind, text, claim, expect) in cases {
            rejects(&b, kind, text, claim, expect);
        }
        let errors = validate(&output("出动右马。", json!([{ "kind": "purpose", "text": "出动右马。", "claim_ids": [] }])), &b, Detail::Normal)
            .unwrap_err();
        assert!(errors.iter().any(|e| e.contains("没有引用任何结论")), "{errors:?}");
        assert!(validate("{\"summary\":", &b, Detail::Normal).unwrap_err()[0].contains("JSON"));
        let long = "很长".repeat(200);
        let errors =
            validate(&output("出动右马。", json!([{ "kind": "purpose", "text": long, "claim_ids": [basis] }])), &b, Detail::Brief)
                .unwrap_err();
        assert!(errors.iter().any(|e| e.contains("篇幅")), "{errors:?}");
    }

    #[test]
    fn numbers_are_left_to_the_program() {
        let b = detailed_bundle(&[("h0g2", 30, &["h0g2", "h9g7"]), ("b0c2", 20, &["b0c2"])], true);
        let cand = claim_of(&b, "candidate");
        let reply = claim_of(&b, "reply");
        let cases = [
            // 评审中的反例：评分正负号反转、候选评分错配、深度写成分差
            ("马二进三的评分为 -30。", cand.as_str(), "出现“30”"),
            ("马八进七的评分是 +30。", cand.as_str(), "出现“30”"),
            ("比第二个候选高约 10 分。", cand.as_str(), "出现“10”"),
            // 用汉字写出的数值
            ("马二进三的评分为负三十。", cand.as_str(), "出现“负三十”"),
            ("当前搜索深度为二十。", cand.as_str(), "出现“二十”"),
            ("评分为三。", cand.as_str(), "出现“三”"),
            ("大约差了十分。", cand.as_str(), "出现“十分”"),
        ];
        for (text, claim, expect) in cases {
            rejects(&b, "comparison", text, claim, expect);
        }
        // 普通的步数说法不是数值结论
        let sections = json!([{ "kind": "reply", "text": "对方下一步可能出车，第一步先补士更稳。", "claim_ids": [reply] }]);
        validate(&output("出动右马。", sections), &b, Detail::Normal).unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn rank_verdict_and_mate_conclusions_are_left_to_the_program() {
        let b = detailed_bundle(&[("h0g2", 30, &["h0g2", "h9g7"]), ("b0c2", 20, &["b0c2"])], true);
        // 评审中的反例：第二候选引用自己的编号，却被称为首选
        let second = b.pack.candidates[1].claim_id.clone();
        rejects(&b, "comparison", "马八进七是本次搜索的首选。", &second, "“首选”属于");
        let cand = claim_of(&b, "candidate");
        for (text, word) in [("马二进三比马八进七更好。", "更好"), ("这步是一步好棋。", "好棋"), ("红方因此领先。", "领先")] {
            rejects(&b, "comparison", text, &cand, &format!("“{word}”属于"));
        }
        // 评审中的反例：引用“本方可以杀棋”的结论，却说本方会被杀；没有将杀结论时说“已经形成将杀”
        let mate = mate_bundle(true);
        let mate_line = mate.pack.claims.iter().find(|c| c.kind == "mate_line").unwrap().id.clone();
        rejects(&mate, "reply", "红方会被杀。", &mate_line, "“杀”属于");
        rejects(&mate, "reply", "这步之后可以形成杀棋。", &mate_line, "“杀”属于");
        let basis = claim_of(&b, "basis");
        rejects(&b, "purpose", "这步已经形成将杀，黑方无法防住。", &basis, "“杀”属于");
        // 程序评价为严重失误时，模型同样不写评价，由程序在依据中显示
        let blunder = blunder_bundle();
        assert_eq!(blunder.pack.verdict.as_ref().unwrap().tone, xq_core::explain::VerdictTone::Blunder);
        let cmp = claim_of(&blunder, "comparison");
        rejects(&blunder, "comparison", "这步是严重失误。", &cmp, "“失误”属于");
    }

    #[test]
    fn incomplete_analysis_offers_no_best_line() {
        let b = detailed_bundle(&[("h2e2", 400, &["h2e2"]), ("h0g2", -100, &["h0g2"])], false);
        let cand = claim_of(&b, "candidate");
        let ok = validate(
            &output("出动右马。", json!([{ "kind": "comparison", "text": "候选比较还没有完成。", "claim_ids": [cand] }])),
            &b,
            Detail::Normal,
        )
        .unwrap();
        assert!(ok.followups.iter().all(|f| f.action != FollowupAction::ShowBest));
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

    fn answer(text: &str, ids: &[&str], check: &str) -> String {
        json!({ "answer": { "text": text, "claim_ids": ids }, "points": [], "check": check }).to_string()
    }

    #[test]
    fn answers_must_cite_fitting_claims_and_keep_hard_conclusions_to_the_program() {
        use crate::llm::ask::QuestionKind;
        let b = ask_bundle(QuestionKind::Reply, true, true);
        let id = |group: &str| b.pack.claims.iter().find(|c| c.group == group).unwrap().id.clone();
        let (reply, mine) = (id("mine_reply"), id("mine"));
        let ok = answer("炮二平五之后，对方可以马8进7保护中卒，需要先看中路的保护关系。", &[&reply], "中卒现在有几个棋子保护？");
        assert!(validate_answer(&ok, &b).is_ok(), "{:?}", validate_answer(&ok, &b));
        // 回答没有引用与问题相符的依据
        let off = answer("炮二平五让炮来到中路。", &[&mine], "中路现在由哪些棋子控制？");
        assert!(validate_answer(&off, &b).unwrap_err().iter().any(|e| e.contains("与问题相符")));
        // 留给学习者的问题同样不能写硬结论、新增着法或数字
        for (check, word) in [("对方还能更好地应对吗？", "更好"), ("能否走车九平八？", "车九平八"), ("这一步的评分有三十分吗？", "三十")] {
            let errors = validate_answer(&answer("对方可以马8进7保护中卒。", &[&reply], check), &b).unwrap_err();
            assert!(errors.iter().any(|e| e.contains(word) || e.contains("数")), "{check}：{errors:?}");
        }
        // 不存在的编号、空的验证问题、过多的要点
        let errors = validate_answer(
            &json!({
                "answer": { "text": "对方可以马8进7。", "claim_ids": ["c999"] },
                "points": [{ "text": "一", "claim_ids": [&reply] }, { "text": "二", "claim_ids": [&reply] }, { "text": "三", "claim_ids": [&reply] }, { "text": "四", "claim_ids": [&reply] }],
                "check": "",
            })
            .to_string(),
            &b,
        )
        .unwrap_err();
        for want in ["不存在的结论编号 c999", "要点最多", "验证的问题"] {
            assert!(errors.iter().any(|e| e.contains(want)), "缺少“{want}”：{errors:?}");
        }
    }

}
