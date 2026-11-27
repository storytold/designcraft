use designcraft_doc::Styles;
use designcraft_doc::mojikumi::{Rules, class};

#[test]
fn middle_dot_keeps_quarter_em_beside_untrimmed_bracket() {
    let styles = Styles::default();
    let rules = Rules::resolve(&styles, "LineEndAllOneHalfEmEnum").unwrap().unwrap();
    let aki = rules.pair_with_bodies(class('”'), class('・'), 0.0, 0.25);
    assert_eq!(aki.desired, 0.25, "”・");
    let aki = rules.pair_with_bodies(class('・'), class('“'), 0.25, 0.0);
    assert_eq!(aki.desired, 0.25, "・“");
    let aki = rules.pair_with_bodies(class('｣'), class('：'), 0.0, 0.25);
    assert_eq!(aki.desired, 0.25, "｣：");
    let aki = rules.pair_with_bodies(class('；'), class('｢'), 0.25, 0.0);
    assert_eq!(aki.desired, 0.25, "；｢");
    let aki = rules.pair_with_bodies(class('」'), class('・'), 0.5, 0.25);
    assert_eq!(aki.desired, 0.25, "」・ trimmed");
}
