use dominion_coding_two_file::{parser::parse_weighted,scorer::highest};

#[test]
fn colon_delimited_input_parses_weighted_labels(){
    assert_eq!(parse_weighted("alpha:17").unwrap(),("alpha".into(),17));
    assert_eq!(parse_weighted(" beta : 4 ").unwrap(),("beta".into(),4));
}
#[test]
fn malformed_input_cannot_silently_receive_a_score(){
    for input in ["alpha",":4","x:-2","x:not-a-number",""]{
        assert!(parse_weighted(input).is_err(),"{input:?} must be rejected");
    }
}
#[test]
fn selects_highest_score_and_deterministic_tie_break(){
    let records=vec![("gamma".into(),3),("beta".into(),20),("alpha".into(),20)];
    assert_eq!(highest(&records).as_deref(),Some("alpha"));
    assert!(highest(&[]).is_none());
}
#[test]
fn parses_and_ranks_real_aggregated_input(){
    let records=["gamma:3","beta:20","alpha:20"].iter()
        .map(|line|parse_weighted(line).unwrap()).collect::<Vec<_>>();
    assert_eq!(highest(&records).as_deref(),Some("alpha"));
}
