# 서브3 — 로컬 모델 실행·관리 책임 분해 (2026-11-13)

`src/engine/models.rs`의 다중 책임을 기존 공개 API를 유지한 채 6개 컴파일 모듈로 나눴다.

- `inventory.rs`: 모델 설치본 및 GGUF/MLX 파일 구조
- `runtime_install.rs`: 런타임 발견·설치
- `registration.rs`: 파일 등록·삭제
- `session.rs`: 실행 중인 모델 상태·로드·언로드
- `catalog.rs`: Hugging Face 메타데이터 조회
- `downloads.rs`: 다운로드 작업·재시작·취소·해시 검증

`rsi_system::source_role`과 `COMPONENTS`를 동시에 고쳐 새 모델 코드가 `algorithms`로 잘못 분류되지 않고, 재컴파일된 후손 설계도의 `models` 영역에서 직접 발견되도록 했다. 소스 계보 해시 및 대표 소스에 관한 반례를 테스트에 추가했다.

기반 모델의 가중치·학습·LoRA 경로, 평가자, 채택 게이트는 수정하지 않았다. 컴파일·빌드·테스트·CI·유료 모델 실험은 실행하지 않았다. 개별 기능과 모델 성능 향상은 별도 평가 대상이다.
