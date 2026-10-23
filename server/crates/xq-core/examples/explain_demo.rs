//! 打印几个样例局面的讲解，便于人工检查文字质量：
//! `cargo run -p xq-core --example explain_demo`
use xq_core::Move;
use xq_core::explain::ExplainContext;
use xq_core::explain::explain_move;
use xq_core::explain::explain_position;
use xq_core::fen::parse_position;

fn main() {
    let ctx = ExplainContext { history_complete: true, repetition_count: 1 };
    let samples = [
        ("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w", "h2e2"),
        ("rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR b", "h9g7"),
        ("rnbakab1r/9/1c4nc1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C4/9/RNBAKABNR w", "e2e6"),
        ("rnbakab1r/9/1c4nc1/p1p1C1p1p/9/9/P1P1P1P1P/1C7/9/RNBAKABNR b", "g7e6"),
        ("2bakab2/9/9/4c4/9/9/9/4C4/3R5/3K5 w", "d1d6"),
    ];
    for (fen, m) in samples {
        let pos = parse_position(fen).unwrap();
        let e = explain_move(&pos, Move::parse_iccs(m).unwrap(), &ctx, None).unwrap();
        println!("== {}", e.summary);
        for c in &e.need { println!("  [需求] {} {:?}", c.text, c.evidence); }
        for c in &e.basis { println!("  [依据] {} {:?}", c.text, c.evidence); }
        for c in &e.comparison { println!("  [比较] {}", c.text); }
        println!("  [学习] {}", e.lesson);
    }
    let p = explain_position(&parse_position("3ak4/4a4/9/9/9/9/9/9/4R4/3K5 w").unwrap(), &ExplainContext { history_complete: false, repetition_count: 1 }, None);
    println!("== {}", p.summary);
    for c in &p.claims { println!("  {}", c.text); }
    for w in &p.warnings { println!("  [注意] {w}"); }
}
