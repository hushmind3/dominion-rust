//! Economic discovery operators. Deliberately a pure, testable strategy layer:
//! no new model runtime, no extra multi-agent token fan-out.
pub const OPERATORS: &[(&str,&str)] = &[
    ("inventor","저렴해진 기술을 고액 고객 문제에 이전하고 기존에 없던 상품을 설계"),
    ("merchant","지역·국가·채널별 공급 가격과 고객 신뢰 격차를 수익구조로 변환"),
    ("platform","보유 재고 없이 거래, 신뢰, 연결, 라이선스와 반복 수수료를 구조화"),
    ("seller","지금 돈을 낼 특정 고객과 구매 장애물, 검증 가능한 첫 제안을 찾기"),
    ("operator","설치·AS·인력·법규·현금회전·모방 위험과 기기 투자 회수기간을 검증"),
];

/// Productive disagreement in one inference call instead of paying 5 models
/// to have an ungrounded conversation.
pub fn panel_directive(mode:&str)->String{
    let focus=match mode {
        "signals"=>"오늘의 새로운 정보가 어떤 가격·기술·공급·규제·고객 행동을 바꿨는가",
        "cross"=>"어느 기술의 잠재고객이 원래 기술 산업 밖에 있는가",
        "challenge"=>"이 아이디어를 내 돈으로 실행한다면 어디서 파산하거나 고객을 잃는가",
        "mutate"=>"다른 고객·구매 상황·계약형태로 바꾸면 이익이 더 커지는가",
        _=>"기존 시장의 비효율을 누가 왜 유지시키고 있으며 그 장애물을 누가 해결할 수 있는가",
    };
    let strategies=OPERATORS.iter().map(|(name,description)|
        format!("- {name}: {description}")
    ).collect::<Vec<_>>().join("\n");
    format!(r#"사업가 판단 전략 포트폴리오:
{strategies}
이들은 유명 인물을 연기하는 페르소나가 아니라 서로 다른 경제적 탐색 함수다.
각 관점은 서로 다른 돈벌이 구조를 제안하고, 반드시 구매자·대체재·고객 획득·자본회전으로 검증한다.
가장 많이 동의한 아이디어를 무조건 채택하지 말고, 소수의 비정형 아이디어도 근거와 실행 가능성이 있으면 남긴다.
이번 회차의 중심 질문: {focus}
관점별 1개 이상의 후보를 검토하되 유효하지 않은 후보를 억지로 결과에 넣지 않는다.
단일 모델 호출 내에서 처리하고 페르소나별 장황한 대화 로그를 생성하지 않는다."#)
}
pub fn discovery_brief(mode:&str)->&'static str{
    match mode{
        "signals"=>r#"뉴스·검색·공개 웹페이지의 '변화'를 원료로 사용한다.
1. web_search로 지난 30일의 중국·한국·미국/글로벌 신기술, 공급 차질, 가격 변동, 법규, 고객 불만, 무료 도구/오픈소스 출시를 조사하라.
2. http_fetch로 원문을 읽어 제목만 보고 단정하지 마라. 검색 결과와 원문이 다르면 원문을 우선하라.
3. 출처·발행일·관측한 변화·수익화 메커니즘을 sourceSignals에 먼저 기록하라.
4. 서로 다른 산업의 변화 2개 이상을 결합해 누가 지금 돈을 지불할지 역으로 설계하라.
5. 일상적인 뉴스 요약이나 쇼핑 트렌드 나열 대신 구매자를 확보할 수 있는 기회만 남겨라."#,
        "cross"=>r#"과거 DB에서 서로 다른 산업의 경제적 마찰을 찾아 2~3개 결합하라.
기술의 원래 고객과 실제 고액 지불 고객을 분리하고 제조·중개·서비스·SaaS·라이선스로 수익구조를 재편하라."#,
        "challenge"=>r#"각 후보에 대해 현금 소진, 과소평가된 노동, 광고비, 인증, 경쟁사 대응과 시장 수요 부재를 공격하라.
실험 비용을 가장 적게 쓰면서 실패를 빨리 발견할 수 있는 검증 방법을 선택하라."#,
        "mutate"=>r#"기존 후보를 출발점으로 고객·가격·포장·판매 방식·기술을 변형한다.
직접 판매가 최선이 아니라면 화이트라벨·연동·중개·로열티 등으로 비용과 자본 노출을 줄여라."#,
        _=>r#"업종에 관계없이 고객이 지금 비싸게 해결하는 문제부터 탐색하라.
기술적 가능성, 지불 능력, 낮은 초기 투자, 실제로 접근 가능한 첫 고객이 교차하는 신규 사업을 발명하라."#
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn personas_are_distinct_and_budget_aware(){
        let p=panel_directive("signals");
        assert!(p.contains("inventor")&&p.contains("operator"));
        assert!(p.contains("자본회전")&&p.contains("단일 모델 호출"));
    }
    #[test] fn news_requires_primary_source_and_cross_market_reasoning(){
        let p=discovery_brief("signals");
        assert!(p.contains("http_fetch")&&p.contains("sourceSignals"));
        assert!(p.contains("원문"));
    }
}
