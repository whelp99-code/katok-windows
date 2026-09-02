# katok-windows

`katok`은 카카오톡 대화를 **내 컴퓨터 안에서만** 읽어서 키워드 검색·BM25 검색·의미(벡터) 검색을 할 수 있게 해주는 CLI 도구입니다. 대화 내용을 서버로 보내지 않습니다.

이 저장소는 원본 [NomaDamas/katok](https://github.com/NomaDamas/katok)(Apple Silicon macOS 전용)을 **Windows에서도 쓸 수 있게 확장**한 포크입니다.

- 검색 엔진(keyword / BM25 / semantic), chunk 분할, 아카이브는 원본과 **완전히 동일**합니다.
- Windows에서는 데이터를 넣는 **입력 방식(소스 어댑터)** 만 다릅니다. 아래에서 자세히 설명합니다.

---

## 먼저 읽어주세요 — 이걸 clone하면 맥처럼 자동으로 되나요?

**아니요.** 이 공개 저장소를 clone해도 macOS katok처럼 "앱을 켜두면 전체 대화가 자동으로 읽혀 검색되는" 것은 **되지 않습니다.**

- **되는 것**: 카카오톡 공식 **"대화 내보내기(.txt)"** 로 뽑은 파일을 색인해서 검색(keyword/BM25/semantic).
- **안 되는 것**: 맥처럼 앱의 로컬 DB를 자동으로 통째 읽는 것. → **의도적으로 넣지 않았습니다.**

왜냐하면 Windows 카카오톡의 대화 DB를 자동으로 읽으려면 앱의 보호(Themida)를 우회해 대화를 강제 복호화·추출해야 하는데, 그렇게 얻는 데이터에는 **그룹채팅 속 다른 사람들의 메시지(동의하지 않은 제3자)** 까지 포함됩니다. 이를 도구로 만들어 배포하는 것은 **통신비밀보호법·저작권법(기술적 보호조치 우회)** 저촉 소지와 제3자 프라이버시 침해가 있어, 이 저장소는 **공식 내보내기 기반의 안전·합법 경로만** 제공합니다.

즉 이 저장소로는 **"내가 내보낸 내 대화를 내 PC에서 검색"** 할 수 있고, 남의 카톡을 몰래 보거나 맥처럼 전체 자동 직독을 하는 용도는 **불가능**합니다. (자세한 배경은 [DEVELOPMENT.md](DEVELOPMENT.md).)

---

## macOS판과 Windows판의 정확한 차이

검색 기능 자체는 두 OS가 똑같습니다. **딱 하나, "메시지를 어떻게 가져오느냐"가 다릅니다.**

| 구분 | macOS (원본) | Windows (이 포크) |
|---|---|---|
| 데이터 소스 | `--source macos` — 앱의 로컬 DB를 **직접 읽음** | `--source txt` — 카카오톡 **"대화 내보내기"** 로 뽑은 `.txt`를 읽음 |
| 자동화 | **완전 자동**. `katok sync` 한 번에 전체 대화방 색인 | **수동**. 원하는 방을 내보내기 → 그 파일을 sync |
| 범위 | 앱에 있는 모든 대화 | 내가 내보낸 방만 |
| 최신화 | 다시 `sync` 하면 최신 반영 | 다시 내보내야 갱신 |

### 왜 Windows는 "자동 직독"이 안 되나? (개발할 때 가장 어려운 부분)

macOS 카카오톡의 로컬 DB는 암호화 방식이 **공개적으로 알려져 있어서**(SQLCipher + 기기 식별자 기반 키) 원본 katok이 자동으로 복호화해 읽습니다.

반면 **Windows 카카오톡은 훨씬 강하게 보호되어 있습니다.** 실제로 분석해 보면:

- 대화 로그 DB(`chat_data\chatLogs_*.edb`)는 별도의 자체 방식으로 **암호화**되어 있고, 키가 앱 실행 파일 안에 숨겨져 있습니다.
- 게다가 `KakaoTalk.exe`는 **Themida**라는 상용 보호기로 **패킹·코드 가상화**되어 있습니다(디스크에서 문자열조차 보이지 않고, 실행 파일의 대부분이 가상머신 바이트코드로 되어 있습니다). 이 때문에 일반적인 정적 분석·디버깅·후킹이 매우 어렵습니다.

즉 macOS는 "문이 잠겨 있지만 열쇠 만드는 법이 공개된" 상태라면, Windows는 "문 자체가 상용 금고로 이중 보호된" 상태입니다. **이 차이가 이 프로젝트에서 가장 어려운 지점이었습니다.**

그래서 이 포크는 Windows에서 **공식 "대화 내보내기" 기능으로 뽑은 .txt를 검색**하는, 안전하고 합법적인 경로를 제공합니다. (앱의 보호를 우회해서 남의 대화까지 긁어오는 도구는 프라이버시·법적 문제로 **이 저장소에 포함하지 않습니다.** 아래 [면책](#면책과-사용-범위) 참고.)

---

## 설치 / 빌드

Rust가 필요합니다([rustup](https://rustup.rs)).

```bash
git clone https://github.com/Hostingglobal-Tech/katok-windows.git
cd katok-windows
cargo build --release
# 결과물: target/release/katok(.exe)
```

Windows에서는 순정 SQLite로 빌드되어 별도 준비물(OpenSSL/perl)이 필요 없습니다. macOS/Linux에서는 원본과 동일하게 SQLCipher 빌드를 씁니다.

---

## Windows 사용법

1. 카카오톡에서 검색하고 싶은 **대화방 → 메뉴 → 대화 내용 → 대화 내보내기(.txt)** 로 저장합니다.
2. 색인하고 검색합니다.

```bash
# 내보낸 txt를 색인
katok sync --source txt "C:\path\to\대화방.txt"

# 검색
katok search keyword "계약서"
katok search bm25 "지난주 미팅 자료"
katok search semantic "최근에 논의한 세금 신고 일정"

# 더 넓은 맥락 보기
katok chunk get <chunk-id>
katok chunk context <chunk-id>
```

여러 방을 넣으려면 방마다 내보내기 → 각각 `sync` 하면 됩니다.

---

## 메시지 보내기 (Windows 공식 앱 UI)

`katok send`는 **이미 실행 중이고 이미 로그인된** 공식 카카오톡 PC(`KakaoTalk.exe`)의 채팅창을 Windows UI(창 포커스 / SendInput / 클립보드 붙여넣기)로 조작합니다. 카카오 프로토콜 클라이언트가 아니고, 로그인 도구도 아닙니다. 이 포크에서 macOS `katok send`는 범위 밖입니다.

```bash
# 1:1 방 제목으로 포커스만 (전송 안 함)
katok send --room 제피란더스 --dry-run --json

# 보내기. 한글은 IME 대신 클립보드 붙여넣기를 씁니다.
katok send --room 제피란더스 --text "안녕하세요"

# txt sync 후 아카이브 chat_id 로 지정
katok send --chat txt-xxxxxxxx --text "안녕하세요" --json
```

전제:

- 공식 카카오톡 PC가 설치되어 있고, **이미 로그인된 상태**로 켜져 있어야 합니다.
- `--room`은 카카오톡이 창/목록에 보여주는 1:1 제목과 같아야 합니다.
- `--chat`은 `sync --source txt` 로 넣은 아카이브의 `chat_id`입니다. 같은 이름이 여러 방이면 `--chat`을 쓰세요.
- `--dry-run`은 방을 열거나 포커스한 뒤 멈춥니다. 입력하거나 Send를 누르지 않습니다.
- 카카오톡이 꺼져 있거나 방을 찾지 못하면 분명한 오류로 끝납니다. 실제 전송 테스트는 `--dry-run`만 사용하세요.

### 지원하는 .txt 형식

- **PC/최신 모바일**: `--------------- 2026년 1월 1일 목요일 ---------------` 날짜 구분선 + `[이름] [오전 9:00] 본문`
- **구형 모바일**: `2026년 1월 1일 오전 9:00, 이름 : 본문`

여러 줄 메시지도 이어 붙이고, 시각은 KST(UTC+9)로 해석해 저장합니다.

---

## macOS 사용법 (원본과 동일)

```bash
katok doctor --json
katok sync --source macos --json
katok index --json
katok search semantic "찾고 싶은 내용" --json
```

macOS 상세는 원본 [NomaDamas/katok](https://github.com/NomaDamas/katok)을 참고하세요.

---

## 검색 방식 (macOS·Windows 공통)

- `search keyword` — 정확한 단어/이름/계좌번호 등 그대로 기억나는 값
- `search bm25` — SQLite FTS5 BM25 랭킹, 여러 단어가 섞인 일반 질의
- `search semantic` — EmbeddingGemma 로컬 벡터 검색, 표현이 정확히 기억나지 않아도 의미로 검색

세 방식 모두 결과는 짧은 snippet + chunk id만 보여주고, 원문 전체는 `katok chunk get`을 명시적으로 실행할 때만 출력합니다.

---

## 이 포크에서 바뀐 것

- `--source txt` 소스 어댑터 추가 (`src/import_txt.rs`)
- Windows 빌드 대응: Windows 타깃은 순정 SQLite(`rusqlite` `bundled`)로 빌드해 OpenSSL/perl 의존 제거
- `katok send`: 공식 `KakaoTalk.exe` UI로 1:1 메시지 전송 (Windows, `--dry-run` 지원)
- 데이터 디렉토리: Windows는 `%APPDATA%\katok`
- 한글 문서

원본의 검색 엔진·chunk·semantic 코드는 그대로 유지했습니다.

---

## 면책과 사용 범위

- 이 도구는 **본인 소유 기기에서, 본인이 정당하게 접근 권한을 가진 대화**를 검색하는 개인용/연구용입니다.
- 타인의 대화를 무단으로 열람·수집하는 것은 대한민국 **통신비밀보호법·정보통신망법** 등에 저촉될 수 있습니다. 절대 그런 용도로 쓰지 마세요.
- 카카오톡 앱의 기술적 보호조치를 우회해 대화 DB를 강제로 복호화·추출하는 코드는 위와 같은 이유로 이 저장소에 포함하지 않습니다. Windows에서는 공식 "대화 내보내기" 기능을 사용하세요.
- 대화 내용은 전부 로컬에만 저장됩니다. 어떤 데이터도 외부로 전송하지 않습니다.

## 라이선스 / 원저작자

MIT. 원본 저작권 및 검색 엔진 구현은 [NomaDamas/katok](https://github.com/NomaDamas/katok)에 있습니다. 이 저장소는 Windows 지원을 더한 포크입니다.

개발 과정에서 시도한 방법들(성공·실패 포함)의 기술 기록은 [DEVELOPMENT.md](DEVELOPMENT.md)를 참고하세요.
