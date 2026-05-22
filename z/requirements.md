# CCX Orchestrator 要件定義書（改訂版 v3 / cmux・WorkExecution 版）

## 1. 目的と前提

本システムは、**CCX Controller**、**Orchestrator Agent**、**Worker Agent**、**cmux workspace** を組み合わせ、単一リポジトリの開発作業を複数作業単位で並列実行する開発支援ツールである。

本改訂版では、従来の「CCX 内部で Task 一覧と Task 状態を正本管理する」前提を廃止し、**ユーザーが指定した task source file を業務上の作業管理の正本**とする。

Controller は、業務上の Task 台帳を正本として持たない。Controller が正本として保持するのは、WorkExecution、AgentSession、worktree、PR、artifact、merge lock、file watcher event などの **execution state** のみである。

本書では曖昧さを避けるため、`Orchestrator` という語を単独では用いない。以下の 3 つを明確に区別する。

- **CCX Controller / Controller**  
  Rust で実装される制御層。execution state、cmux / tmux adapter、AgentSession、worktree、file watcher、hook、PR、merge lock、永続化、CLI を管理する。

- **Orchestrator Agent**  
  task source file の読解・更新、作業対象の選定、個別 task file の作成、AgentSession 制御、並列実行制御、失敗時判断、復旧判断、マージ責任、follow-up 反映を担う AI エージェント。

- **Worker Agent**  
  Orchestrator Agent から渡された 1 つの WorkExecution に対して、実装、検証、サブエージェントレビュー、修正反復、commit、push、PR 作成、`gh-review-hook` 反復、マージ可能判断を担う AI エージェント。

本システムの基本方針は以下とする。

- 作業一覧、完了印、残 Task、補足説明、優先度メモなどの業務上の情報は task source file を正本とする。
- Orchestrator Agent のみが task source file を編集してよい。
- Worker Agent は task source file を編集しない。
- Orchestrator Agent は WorkExecution ごとに個別 task file を作成し、Worker Agent へ渡す。
- Worker Agent は個別 task file を読み、進捗、PR、レビュー結果、`gh-review-hook` 結果、残作業、差し戻し理由を同ファイルへ追記・更新する。
- Controller は task source file と個別 task file の変更を file watcher で検知し、Orchestrator Agent へ通知する。
- Worker Agent は `gh-review-hook` exit 0 により対象 PR をマージ可能と判断し、`merge_ready` を報告する。
- マージ責任主体は Orchestrator Agent とする。
- 実際の `gh pr merge --merge`、merge lock、PR head 確認、`master` pull、execution state 更新は Controller CLI が機械的に実行してよい。
- UI / terminal workspace は cmux を基本とし、CCX GUI は cmux の custom tab / custom surface として実装することを優先する。
- ロジックは Rust Controller 側に寄せる。

対象プラットフォームは macOS を先行対応とする。Windows / WSL 対応は将来拡張とする。

MVP では対象ブランチは `master` 固定とする。`base_branch` の設定化は MVP では対象外とする。

予算管理機能は実装しない。

---

## 2. MVP Scope

MVP では以下を対象とする。

- macOS
- cmux workspace / cmux tab 連携
- Rust Controller
- Controller CLI を主経路とする control interface
- MCP は MVP では必須としない
- 1 project = 1 cmux workspace
- 1 AgentSession = 1 cmux tab
- 1 AgentSession = 1 tmux session
- 1 WorkExecution = 1 branch / 1 worktree / 0..1 PR / 1 個別 task file
- 1 WorkExecution に複数 AgentSession を attach 可能
- 同一 worktree の active writer は最大 1 AgentSession
- reviewer / observer / diagnostic AgentSession は read-only attachment として複数可
- repo 外 worktree
- `master` 固定
- task source file は Markdown を主対象とするが形式固定しない
- Worker Agent は `merge_ready` 報告までを担当
- Orchestrator Agent がマージ責任を持つ
- Controller CLI が機械的 merge execute を実行してよい
- `gh-review-hook` は対象 WorkExecution の worktree を cwd として実行する

MVP の CLI コマンド名・主要引数は実装設計書で定義する。ただし、外部公開 API としての安定化は MVP 後に行う。

---

## 3. 背景と運用モデル

ユーザーはこれまで、概ね次の手順を手動で実行している。

1. task source file を読む。
2. 機械的に検証可能な作業単位を 1 つ進める。
3. 実装する。
4. サブエージェントでレビューする。
5. 指摘がなくなるまで修正する。
6. PR を作成する。
7. `gh-review-hook` を実行する。
8. 指摘が出なくなるまで修正、commit、push、`gh-review-hook` を繰り返す。
9. `gh-review-hook` が exit 0 になったらマージ可能と判断する。
10. マージする。
11. `master` を checkout / pull する。
12. task source file を更新する。
13. 次の作業へ進む。

本システムでは、この手動フローを次のように並列化する。

|工程|担当|
|---|---|
|task source file の読解、優先順位判断|Orchestrator Agent|
|個別 task file の作成|Orchestrator Agent|
|AgentSession / cmux tab / worktree / branch の作成|Controller|
|Worker Agent への実作業指示|Orchestrator Agent から prompt 投入|
|実装、検証、サブエージェントレビュー、PR 作成|Worker Agent|
|`gh-review-hook` の反復実行と修正判断|Worker Agent|
|`gh-review-hook` exit 0 後のマージ可能判断|Worker Agent|
|個別 task file の更新|Worker Agent|
|個別 task file 更新の検知|Controller file watcher|
|マージするか、follow-up するか、差し戻すかの判断|Orchestrator Agent|
|merge lock、PR head 確認、`gh pr merge --merge`、`master` pull|Controller CLI、責任主体は Orchestrator Agent|
|task source file への完了反映、残 Task 反映|Orchestrator Agent|
|Worker Agent / worktree cleanup|Controller、Orchestrator Agent の判断に基づく|

直列実行時の「上から順」は運用例であり、並列実行時の固定ルールではない。並列実行時の着手順、優先順位、同時実行数、依存関係考慮は、Orchestrator Agent が task source file、ユーザー入力、execution state、merge queue、Worker の空き状況を踏まえて判断する。

---

## 4. 用語定義

- **Project**  
  1 つの canonical repo と CCX 実行状態をまとめる単位。1 Project は 1 cmux workspace に対応する。

- **project_id**  
  Project 登録時に生成する ULID。canonical repo path 由来の slug は表示用に分離する。

- **display_slug**  
  canonical repo path の `/` を `-` に置換するなどして作る表示名。ID としては扱わない。

- **task source file**  
  ユーザーが指定する作業管理ファイル。Markdown を主に想定するが、MVP では形式固定しない。業務上の作業一覧、完了印、残 Task、補足説明などの正本である。

- **work item**  
  task source file 上の 1 つの作業単位。見出し、チェックボックス項目、リスト項目、アンカーコメント、行範囲、抜粋、またはユーザーが明示した範囲を含む。

- **work_item_ref**  
  work item を参照するための情報。`source_path`、`selector_type`、`selector_value`、`display_text`、必要に応じて file hash や source snapshot を持つ。

- **WorkExecution**  
  1 つの work item を実行する論理単位。1 WorkExecution は 1 branch、1 worktree、0..1 PR、1 個別 task file を持ち、複数 AgentSession を紐づけられる。

- **work_execution_id**  
  WorkExecution に付与される ULID。

- **AgentSession**  
  Orchestrator Agent、Worker Agent、Reviewer Agent、Diagnostic Agent などの 1 実行セッション。1 AgentSession は 1 cmux tab と 1 tmux session に対応する。

- **agent_session_id**  
  AgentSession に付与される ULID。

- **cmux workspace**  
  Project ごとの UI / terminal workspace。1 Project = 1 cmux workspace。

- **cmux tab**  
  AgentSession ごとの UI タブ。1 AgentSession = 1 cmux tab。

- **tmux session**  
  AgentSession ごとに Controller が必要に応じて作成する実行基盤。MVP では 1 AgentSession = 1 tmux session とする。

- **Task Harness / Harness**  
  Controller が管理する AgentSession 実行管理単位。PID、cwd、環境変数、hook、task file path、heartbeat、lifecycle event を管理する。

- **canonical repo**  
  ユーザーが開いた元のリポジトリ作業領域。MVP では task source file の読み書き、`master` の pull、マージ後の同期確認に用いる。

- **worktree**  
  WorkExecution に紐づく repo 外 Git 作業領域。コード編集、commit、push、PR 作成、`gh-review-hook` 実行の cwd とする。

- **worktree attachment**  
  AgentSession が WorkExecution の worktree に関与する関係。`writer`、`reviewer`、`observer`、`diagnostic` の mode を持つ。

- **write lease**  
  同一 worktree に対する同時 write を 1 AgentSession に限定するための lease。writer attachment にのみ付与される。

- **個別 task file / run-local task file**  
  WorkExecution ごとに作成される `task.md`。Orchestrator Agent が作成し、Worker Agent が進捗・結果・PR・レビュー・残作業を追記する。MVP では Worker との主要な入出力ファイルかつ状態管理の正本とする。

- **merge_ready**  
  Worker Agent が `gh-review-hook` exit 0 を確認し、PR をマージ可能と判断した状態。

- **merge_lock**  
  Project 内でマージ処理を同時に 1 つだけ実行するための排他ロック。

- **file watcher event**  
  Controller が task source file または個別 task file の変更を検知したイベント。

- **agent lifecycle hook / exit hook**  
  Agent 実装が応答終了、停止、終了などのタイミングで発火する hook。shell process の終了とは限らない。

- **user_intervention**  
  ユーザーが Orchestrator Agent または Worker Agent に直接入力したイベント。Agent の自律判断や過去の自動指示より優先される。

- **gh-review-hook**  
  PR の外部レビュー結果、bot 指摘、CI、required check、conflict 状態などを確認し、終了コードを返す hook。

---

## 5. ドメインモデル

MVP の主要関係は以下とする。

```text
Project 1 ── 1 cmux workspace
Project 1 ── * WorkExecution
Project 1 ── * AgentSession
Project 1 ── * Event
Project 1 ── 0..1 MergeLock

WorkExecution 1 ── 1 work_item_ref
WorkExecution 1 ── 1 branch
WorkExecution 1 ── 1 worktree
WorkExecution 1 ── 1 task.md
WorkExecution 1 ── 0..1 PullRequest
WorkExecution 1 ── * AgentSession
WorkExecution 1 ── 0..1 active write lease

AgentSession 1 ── 1 cmux tab
AgentSession 1 ── 1 tmux session
AgentSession 1 ── 0..1 WorkExecution attachment
```

`Run` という語は、MVP では混乱を避けるため原則として `WorkExecution` に置き換える。過去要件の `Run` は、特に断りがない限り `WorkExecution` または `AgentSession` のどちらかへ分解して扱う。

---

## 6. システム構成

|構成要素|責務|
|---|---|
|cmux workspace|Project ごとの UI / terminal workspace。|
|CCX custom tab / surface|CCX の Overview、WorkExecution、AgentSession、PR、merge、events 表示。|
|cmux tab|各 AgentSession の shell 表示と入力。|
|tmux adapter|AgentSession ごとの tmux session 作成、入出力、停止、cleanup を隠蔽する。|
|CCX Controller|Rust 実装。execution state、file watcher、worktree、write lease、merge lock、hook、永続化、CLI を管理する。|
|Orchestrator Agent Session|task source file を読み、WorkExecution を作成し、AgentSession を制御し、merge / follow-up / cleanup を判断する。|
|Worker Agent Session|WorkExecution の writer として実装・レビュー修正・PR 作成・hook 反復・merge_ready 報告を行う。|
|Reviewer / Diagnostic Agent Session|必要に応じて同一 WorkExecution に read-only attachment される補助 AgentSession。|
|Review Integration|`gh-review-hook` を通じて外部レビュー、CI、required check、conflict 状態を集約する。|
|Persistence Layer|JSONL 監査ログと SQLite execution state view を管理する。|

Orchestrator Agent / Worker Agent はともに素の shell として起動する。入力は shell の stdin をそのまま使う。Controller は shell の操作感を改変せず、cmux / tmux adapter、file watcher、hook、prompt 投入、CLI により実行単位を管理する。

特定 Agent 実装向けの専用ラッパーは必須としない。互換性問題が出た場合のみ adapter 層で fallback を提供する。

---

## 7. cmux / tmux / GUI 要件

### 7.1 基本方針

MVP では、UI / terminal workspace は cmux に寄せる。

- 1 Project は 1 cmux workspace に対応する。
- 1 AgentSession は 1 cmux tab に対応する。
- 1 AgentSession は 1 tmux session に対応する。
- 複数 AgentSession を同一 cmux tab に同居させない。
- 複数 AgentSession を同一 tmux session に同居させない。
- Worker Agent の cmux tab は、対応する worktree を cwd とする。
- Orchestrator Agent の cmux tab は canonical repo または Controller が指定する orchestration cwd を使用する。
- GUI は独立アプリよりも、cmux の custom tab / custom surface として実装することを優先する。
- ロジックは Rust Controller 側に置く。

### 7.2 表示領域

CCX custom tab / surface は少なくとも以下を表示する。

|領域|内容|
|---|---|
|Overview|Project 状態、Orchestrator Agent 状態、WorkExecution 数、AgentSession 数、merge_ready 数、hold 数。|
|Task Source|task source file path、最終更新、file watcher event、Orchestrator Agent の反映履歴。|
|WorkExecutions|各 WorkExecution の state、branch、worktree、PR、task.md、active writer。|
|AgentSessions|cmux tab、tmux session、role、attach mode、heartbeat、state。|
|Reviews|`gh-review-hook` exit code、exit 2 stderr、review / CI / conflict 状態。|
|Merges|merge_ready queue、merge_lock、merge execute 履歴、canonical repo の `master` 状態。|
|Artifacts|task.md、diff 要約、commit、PR、follow-up。|
|Events|user_intervention、orchestrator_instruction、worker_update、file_watcher_event、controller_event。|

Shell / cmux tab は閲覧専用ではなく、ユーザーが実際に入力できなければならない。

---

## 8. ID / ディレクトリ設計

### 8.1 ID 方針

ID は ULID を基本とする。

- `project_id`: Project 登録時に生成する ULID。
- `work_execution_id`: WorkExecution 作成時に生成する ULID。
- `agent_session_id`: AgentSession 作成時に生成する ULID。
- `event_id`: JSONL event 作成時に生成する ULID。可能であれば monotonic ULID とする。
- `merge_lock_id`: merge_lock 取得時に生成する ULID。
- `write_lease_id`: write lease 取得時に生成する ULID。

`project_id` は path slug ではなく ULID とする。canonical repo path から生成した slug は `display_slug` としてのみ使う。

### 8.2 Project metadata

Project metadata は以下を持つ。

```json
{
  "project_id": "01HY...",
  "display_slug": "Users-xpadev-src-myrepo",
  "canonical_repo": "/Users/xpadev/src/myrepo",
  "task_source_file": "/Users/xpadev/src/myrepo/z/tasks.md",
  "gh_review_hook": {
    "command": "./gh-review-hook",
    "timeout_seconds": 300
  },
  "cleanup_policy": "keep_last_n",
  "created_at": "2026-05-22T10:00:00+09:00"
}
```

### 8.3 ディレクトリ構成

既定の保存先は以下とする。

```text
~/.ccx/projects/<project_id>/
  project.json
  work-executions/
    <work_execution_id>/
      task.md
      metadata.json
      sessions/
        <agent_session_id>/
          transcript.log
          artifacts/
  worktrees/
    <work_execution_id>/
  events/
    events.jsonl
  state/
    state.sqlite
  locks/
```

実装上は `CCX_HOME` または設定値により配置先を変更できる。

---

## 9. task source file 要件

### 9.1 基本方針

作業管理の業務上の正本は、ユーザーが指定した task source file とする。

task source file は Markdown を主に想定するが、MVP では厳密なファイル形式を固定しない。

Orchestrator Agent は task source file を読み、必要に応じて完了印、残 Task、分割結果、補足説明、anchor を更新してよい。

Worker Agent は task source file を編集してはならない。

### 9.2 work_item_ref

Orchestrator Agent は work item を WorkExecution に切り出す際、`work_item_ref` を作成する。

`work_item_ref` は以下を持つことを推奨する。

- `source_path`
- `selector_type`: `heading|checkbox|list_item|anchor|line_range|excerpt`
- `selector_value`
- `display_text`
- `source_file_hash`
- `selected_at`
- `excerpt_before`
- `excerpt_after`

Orchestrator Agent は必要に応じて task source file に次のような anchor を挿入してよい。

```md
<!-- ccx:work-item 01HY... -->
```

anchor は必須ではない。anchor が存在する場合、`selector_type: anchor` を優先する。

### 9.3 task source file の変更通知

Controller は task source file を file watcher で監視する。

task source file に変更があった場合、Controller は `task_source_file_changed` event を記録し、Orchestrator Agent に通知する。

変更原因は以下を含む。

- ユーザーが外部エディタで直接編集した。
- Orchestrator Agent が完了反映、残 Task 追加、anchor 挿入を行った。
- Git 操作により task source file が更新された。

Controller は task source file の業務的意味を解釈しない。Orchestrator Agent が再読込し、必要に応じて計画を更新する。

### 9.4 dirty state と canonical repo sync の分離

task source file が canonical repo 内にある場合、Orchestrator Agent による更新は canonical repo の dirty state を生む可能性がある。

`gh pr merge` が成功した時点で、PR merge 自体は成功と扱い、`pr_merged` 相当のイベントが発行される。その後の canonical repo sync は別ステップとし、Controller は `WorkExecution` に `state = 'merged'` を設定する。その際、マージ後の sync 状態を正確に管理するために、`sync_status`（値は `pending | success | aborted`）を独立したフィールドとして管理する。

具体的には、Controller は以下のように同期処理を管理する。

- **canonical repo が clean の場合**:
  `git checkout master && git pull --ff-only` を実行し、成功したら `sync_status = 'success'` とする。
- **dirty state が task source file のみの場合**:
  automatic pull は行わず、WorkExecution の `state = 'merged'`, `sync_status = 'pending'`, `sync_warning = 'task_source_dirty'` を記録し、Orchestrator Agent に通知して sync を pending（保留）にする。
- **dirty state が task source file 以外を含む場合**:
  automatic pull は行わず、WorkExecution の `state = 'merged'`, `sync_status = 'aborted'`, `sync_warning = 'dirty_files_detected'` を記録し、Orchestrator Agent に hold / ユーザー介入判断を求める。

これにより、PR merge 成功後に canonical repo pull が失敗またはスキップされても、PR merge 自体の成功状態（`state = 'merged'`) を壊さずに正確かつ安全に管理できる。

---

## 10. 個別 task file 要件

### 10.1 基本方針

WorkExecution ごとに個別 task file `task.md` を作成する。

`task.md` は Orchestrator Agent と Worker Agent の主要な入出力ファイルである。

- Orchestrator Agent が作成する。
- Worker Agent は `task.md` を読み、作業を進める。
- Worker Agent は進捗、PR、review、`gh-review-hook` 結果、残作業、差し戻し理由を `task.md` に追記・更新する。
- Controller は `task.md` の変更を file watcher で検知する。
- Orchestrator Agent は `task.md` の変更通知を受け、merge、follow-up、hold、split、reassign を判断する。

MVP では `task.md` を WorkExecution の主要成果物かつ状態管理の正本とする。

### 10.2 配置

`task.md` の既定配置は以下とする。

```text
~/.ccx/projects/<project_id>/work-executions/<work_execution_id>/task.md
```

### 10.3 推奨 front matter

`task.md` は Markdown を標準とする。本文は自由形式でよいが、Controller と Orchestrator Agent が扱いやすいよう、front matter を付与することを推奨する。

```md
---
project_id: 01HY...
work_execution_id: 01HZ...
status: assigned
source_path: /path/to/tasks.md
source_ref: ccx:work-item:01HZ...
branch: ccx/01HZ/fix-auth
pr_number:
pr_url:
head_commit:
gh_review_hook_exit_code:
current_writer_session_id:
updated_by: orchestrator
updated_at: 2026-05-22T10:00:00+09:00
---

# Work Item

## Original Task

Orchestrator Agent が切り出した作業内容。

## Instructions

Worker Agent への具体指示。

## Progress

Worker Agent が進捗を追記する。

## Pull Request

PR 番号、URL、branch、head commit。

## Review / Gate

サブエージェントレビュー、`gh-review-hook` の結果。

## Result

Worker Agent の最終結果。

## Remaining Work

残作業や follow-up。

## Blockers

詰まり、差し戻し理由、分割提案。
```

### 10.4 status

`task.md` の `status` は少なくとも以下を許容する。Controller は `task.md` の front matter `status` を補助メタデータとして読み取り、`WorkExecutionState` と以下のように対応させる。

| task.md status | 対応する WorkExecutionState | 意味 |
|---|---|---|
| `assigned` | `task_file_created` | Orchestrator Agent が作成し、まだ Worker 作業前。 |
| `working` | `running` | Worker Agent が作業中。 |
| `pr_open` | `pr_open` | PR が作成された。 |
| `gate_check` | `gate_check` | `gh-review-hook` 実行中または再実行待ち。 |
| `review_fixing` | `review_fixing` | レビュー / CI / conflict 指摘へ対応中。 |
| `merge_ready` | `merge_ready` | Worker Agent がマージ可能と判断した。 |
| `returned` | `returned` | Worker Agent が作業せず差し戻した。 |
| `blocked` | `blocked` | 外部要因や依存関係により進められない。 |
| `failed` | `failed` | Worker Agent が失敗を報告した。 |
| `followup_required` | `followup_required` | 作業後に残 Task / 追加対応が必要。 |
| `merged` | `merged` | Orchestrator Agent がマージ完了を反映した。 |

*※ `created`, `dispatched`, `merging`, `hold`, `canceled`, `superseded` の状態は、Controller / Orchestrator 操作により直接決定される制御状態であり、Worker Agent が `task.md` から直接設定する通常 status ではない。*

### 10.5 file watcher

Controller は `task.md` を file watcher で監視する。

`task.md` に変更があった場合、Controller は `work_execution_task_file_changed` event を記録し、Orchestrator Agent に通知する。

**重複イベント・ノイズ抑制（Deduplication）ルール**:
file watcher は `mtime` のみの更新やエディタによる冗長な書き込みイベントを検知しやすいため、以下のノイズ抑制仕様を強制する。
- **Hash-based Deduplication**: Controller は各 `task.md` の直近のコンテンツハッシュ（SHA-256など）を `last_seen_hash` としてメモリ上に保持する。
- **Skipping Identical Hashes**: ファイル変更検知時、コンテンツのハッシュが `last_seen_hash` と同一であれば、`mtime` やメタデータのみの変化であってもイベントの記録および Orchestrator への通知をスキップする。
- **Priority-based Notification**: front matter の `status` に変更がない場合は、通知の優先度（priority）を下げる、または不要な通知をスキップして Orchestrator の無駄な再処理を防止する。

Controller は本文の意味解釈を行わない。ただし front matter が存在する場合、`status`、`pr_number`、`head_commit`、`gh_review_hook_exit_code` などを補助情報として読み取ってよい。

---

## 11. WorkExecution / worktree / AgentSession の関係

### 11.1 基本方針

MVP では、worktree と Worker Agent Session を 1:1 に固定しない。

1 つの WorkExecution は、1 つの branch、1 つの worktree、0 または 1 つの PR、1 つの `task.md` を持つ。

1 つの WorkExecution には、複数の AgentSession を紐づけてよい。

AgentSession は一時的な実行主体であり、ハング、停止、レビュー、再割当、復旧、診断により増減できる。

同一 worktree に対して同時に write 権限を持つ AgentSession は最大 1 つとする。

### 11.2 attachment mode

AgentSession は WorkExecution に attach できる。

attach mode は以下とする。

|mode|権限|
|---|---|
|`writer`|コード変更、commit、push、PR 更新、`gh-review-hook` 実行が可能。|
|`reviewer`|原則 read-only。レビュー結果を `task.md` に記録できる。|
|`observer`|閲覧、状態確認のみ。|
|`diagnostic`|調査用。write lease がない限り commit / push してはならない。|

`writer` は同一 worktree に対して同時に最大 1 つのみ許可する。

`reviewer`、`observer`、`diagnostic` は複数同時に許可できる。

*※ MVP における read-only attachment (`reviewer`, `observer`, `diagnostic`) は、Controller の write lease 管理、および prompt や運用規約による論理的・ポリシー制約であり、OS レベルのファイルシステム権限による完全な書き込みロックや読み取り専用クローンの強制は必須としない。*

### 11.3 write lease

Controller は worktree ごとに write lease を管理する。

write lease は以下を持つ。

- `write_lease_id`
- `project_id`
- `work_execution_id`
- `worktree_path`
- `writer_agent_session_id`
- `acquired_at`
- `last_heartbeat_at`
- `lease_state`: `active|stale|released|revoked`

write lease を保持していない AgentSession は、当該 worktree に対してコード変更、commit、push、branch 操作を行ってはならない。

writer AgentSession がハング、消失、停止、または明示的に detach された場合、Controller は write lease を stale または revoked として扱う。

Orchestrator Agent は stale lease を確認し、必要に応じて別 AgentSession へ write lease を移譲できる。

Controller は、Orchestrator Agent の判断なしに stale lease を自動移譲してはならない。

### 11.4 ハング時の考え方

AgentSession のハングや消失は、WorkExecution の失敗を直ちに意味しない。

worktree、branch、PR、`task.md` が復旧可能である場合、Orchestrator Agent は新しい AgentSession を同一 WorkExecution に attach し、作業を継続させてよい。

---

## 12. Git worktree / Branch / PR 要件

### 12.1 worktree 基本方針

複数 WorkExecution の並行作業には Git worktree を用いる。

各 WorkExecution は、原則として専用の Git branch と専用の worktree を持つ。

worktree は repo 外に配置する。

既定の配置は以下とする。

```text
~/.ccx/projects/<project_id>/worktrees/<work_execution_id>/
```

既定の branch 名は以下とする。

```text
ccx/<work_execution_id>/<slug>
```

MVP では merge 対象 branch は `master` 固定とする。

### 12.2 canonical repo

canonical repo は、ユーザーが開いた元のリポジトリ作業領域である。

canonical repo は以下に用いる。

- Orchestrator Agent による task source file の読み書き。
- Controller / Orchestrator Agent による `master` の checkout / pull。
- merge 後の同期確認。

Worker Agent は canonical repo を作業 cwd として使わない。

### 12.3 worktree 作成と再利用

Controller は WorkExecution 作成時に branch と worktree を用意する。

- 新規 WorkExecution の場合、新しい branch と worktree を作成する。
- 同じ WorkExecution に AgentSession を追加 attach する場合、既存 branch / worktree を再利用する。
- reassign / recovery の場合、旧 writer lease を stale / revoked / released にしたうえで、同じ branch / worktree に新しい writer AgentSession を attach できる。
- split の場合、既存 WorkExecution を `hold` または `superseded` とし、新しい work_item_ref ごとに新しい WorkExecution / branch / worktree を作成できる。
- merged / canceled / superseded の WorkExecution に紐づく worktree は、設定に従い保持または削除できる。

### 12.4 Worker Agent の branch 制約

Worker Agent は、自身が attach された WorkExecution の branch 上で作業する。

Worker Agent は、通常運用では Worker 用 worktree 内で `master` を checkout してはならない。

Worker Agent は、通常運用では Worker 用 worktree 内で `master` を pull してはならない。

Worker Agent が最新 `master` への追従を必要とする場合、Orchestrator Agent または Controller に依頼し、Controller が安全な手順で fetch / rebase / merge 等を管理する。

### 12.5 差し戻し時の未コミット差分

Worker Agent が作業せずに差し戻す場合、未コミット差分は廃棄してよい。

差し戻し時に未コミット差分が存在する場合、Controller は Orchestrator Agent の判断に基づき以下を実行できる。

```bash
git reset --hard
git clean -fd
```

廃棄前に、Controller は可能な範囲で diff の要約、変更ファイル一覧、直近コマンドを監査ログに保存する。

### 12.6 PR ルール

PR は原則として 1 WorkExecution につき 1 本とする。

同一 WorkExecution に複数 AgentSession が attach されても、branch / PR は同一のものを引き継ぐ。

retry、reassign、reviewer attach、recovery attach によって新しい AgentSession が作られても、原則として同じ branch / PR を継続使用する。

Worker Agent は実装完了後、commit、push、PR 作成までを実行できる。PR 作成後も、writer AgentSession または後続 writer AgentSession が review 修正、`gh-review-hook`、`merge_ready` 報告まで担当する。

---

## 13. Worker Agent 実行要件

### 13.1 基本実行ループ

Worker Agent は割り当てられた WorkExecution に対し、実装から `merge_ready` 報告までを自律的に実行する。

1. `task.md` を読む。
2. work item と付随コンテキストを理解する。
3. work item が大きすぎる、曖昧すぎる、または独立に完結できないと判断した場合、作業せず `task.md` に差し戻し理由を記載する。
4. 実装、build、test、lint を行う。
5. サブエージェントレビューを行う。
6. 指摘がなくなるまで修正を反復する。
7. こまめに commit する。
8. PR を作成する。
9. `gh-review-hook` を対象 worktree cwd で実行する。
10. `gh-review-hook` が exit 2 の場合、stderr のレビュー・CI・conflict 等の出力を読み、必要な修正、commit、push、再実行を繰り返す。
11. `gh-review-hook` が exit 0 になった場合、PR をマージ可能と判断し、`task.md` の `status` を `merge_ready` に更新する。
12. 残作業や follow-up がある場合、`task.md` に記載する。

Worker Agent は、通常運用では以下を行わない。

- `gh pr merge` などのマージ実行。
- Worker 用 worktree 内での `master` checkout。
- Worker 用 worktree 内での `master` pull。
- task source file の編集。

### 13.2 作業前差し戻し

Worker Agent は、自身の作業対象が次のいずれかに該当すると判断した場合、実装を始めず Orchestrator Agent へ差し戻せる。

- 作業範囲が大きすぎる。
- 指示が曖昧すぎる。
- 依存作業が未完了である。
- 1 PR で閉じるには範囲が広すぎる。
- 機械的に検証できない部分が大きい。

差し戻し時、Worker Agent は `task.md` に以下を記載する。

- `status: returned`
- 差し戻し理由。
- 推奨する分割案。
- 作業を進めていないこと、または未コミット差分を廃棄してよいこと。
- 元の `work_item_ref`。

### 13.3 残 Task 返却

Worker Agent は、PR に含めない残作業を Orchestrator Agent へ返してよい。

残作業は `task.md` に記載する。Orchestrator Agent はその内容を読み、必要に応じて task source file へ反映する。

### 13.4 reviewer / diagnostic AgentSession

Orchestrator Agent は必要に応じて、同一 WorkExecution に reviewer / diagnostic AgentSession を attach できる。

reviewer AgentSession は既定では read-only attachment とし、コード変更や commit / push を行わない。

reviewer AgentSession が修正を行う必要がある場合、Orchestrator Agent は既存 writer lease を解放または停止し、reviewer AgentSession に writer lease を移譲できる。

---

## 14. マージ判断とマージ処理

### 14.1 基本方針

マージ判断とマージ処理を分離する。

|項目|担当|
|---|---|
|PR がレビュー・CI・conflict 観点でマージ可能かの判断|Worker Agent|
|`gh-review-hook` の反復実行|Worker Agent|
|`gh-review-hook` exit 0 後の `merge_ready` 報告|Worker Agent|
|merge するか、follow-up / hold / 差し戻しにするかの責任判断|Orchestrator Agent|
|merge lock 取得|Controller CLI、責任主体は Orchestrator Agent|
|PR head や PR 状態の機械的確認|Controller CLI|
|`gh pr merge --merge` 等の実コマンド|Controller CLI|
|merge 後の `master` checkout / pull|Controller CLI|
|execution state の更新|Controller|
|task source file の完了反映|Orchestrator Agent|
|WorkExecution cleanup 判断|Orchestrator Agent|

Orchestrator Agent は、Worker Agent が `merge_ready` と判断した PR について、マージ処理の責任主体となる。

MVP では、実際の `gh pr merge --merge`、PR head SHA 確認、`gh-review-hook` 再実行、canonical repo の `master` pull、execution state 更新、merge lock 管理は、Controller CLI の `merge execute` 相当機能が機械的に実行してよい。

### 14.2 merge_ready 報告

Worker Agent は `gh-review-hook` exit 0 を確認した場合、`task.md` に少なくとも以下を記載する。

- `status: merge_ready`
- `work_execution_id`
- `work_item_ref`
- PR 番号または PR URL
- branch 名
- head commit SHA
- `gh-review-hook` exit 0 を確認したこと
- サブエージェントレビュー結果の要約
- 実行した test / lint / build の要約
- 残作業があればその内容

### 14.3 merge_lock

複数 PR のマージ処理は Project 単位で直列化する。

Orchestrator Agent は PR のマージ処理前に Controller CLI 経由で `merge_lock` を取得する。

`merge_lock` を取得できない場合、対象 WorkExecution は `merge_ready` のまま待機する。

merge lock は以下を持つ。

- `merge_lock_id`
- `project_id`
- `owner_agent_session_id`
- `work_execution_id`
- `pr_number`
- `acquired_at`
- `last_heartbeat_at`
- `state`: `active|stale|released`

復旧時、lock owner が存在しない場合、Controller は merge lock を stale として扱い、Orchestrator Agent に判断を求める。

### 14.4 マージ処理手順

Orchestrator Agent は、`task.md` の `merge_ready` 更新通知を受けた後、必要に応じて以下を実行する。

1. `task.md`、PR 状態、WorkExecution state を確認する。
2. follow-up や blocker がある場合、merge せず Worker Agent へ差し戻すか hold する。
3. merge 可能と判断した場合、Controller CLI の `merge execute` 相当機能を呼び出す。
4. Controller は Project の `merge_lock` を取得する。
5. Controller は対象 PR が存在し、open であることを確認する。
6. Controller は Worker Agent が報告した PR head commit SHA と、現在の PR head commit SHA が一致することを確認する。
7. 一致しない場合、マージせず Orchestrator Agent に通知する。
8. 必要に応じて、Controller は対象 WorkExecution の worktree を cwd として `gh-review-hook` を再実行する。
9. `gh-review-hook` を再実行して exit 2 が返った場合、stderr を Orchestrator Agent に返す。Orchestrator Agent は Worker Agent に差し戻す。
10. 機械的前提が満たされている場合、Controller は merge commit 方式で PR を merge する。
11. Controller は canonical repo の master 同期（sync）を試みる。
    - PR merge 自体が成功した時点で、WorkExecutionState は `merged` に遷移する。
    - canonical repo が clean な場合、`git checkout master && git pull --ff-only` を実行し、成功したら `sync_status = 'success'` とする。
    - dirty state が task source file のみの場合、自動 pull は行わず、`sync_status = 'pending'`, `sync_warning = 'task_source_dirty'` を記録し、Orchestrator Agent に通知して sync を pending（保留）にする。
    - dirty state が task source file 以外を含む場合、自動 pull は行わず、`sync_status = 'aborted'`, `sync_warning = 'dirty_files_detected'` を記録して Orchestrator Agent に hold / ユーザー介入判断を求める。
12. Controller は execution state (`state = 'merged'`, `sync_status`, `sync_warning` 等) を更新する。
13. Orchestrator Agent は task source file に完了反映、残 Task 反映、follow-up 追加を行う。
14. Controller は `task.md` に `status: merged` を反映できる。
15. Controller は `merge_lock` を解放する。
16. Orchestrator Agent の判断に基づき、Controller は当該 WorkExecution の AgentSession / cmux tab / tmux session / worktree を cleanup する。

### 14.5 マージ失敗時

|失敗理由|扱い|
|---|---|
|PR head SHA が変化した|Worker Agent に再確認させ、`gate_check` へ戻す。|
|`gh-review-hook` exit 2|stderr を Worker Agent に渡し、`review_fixing` または `gate_check` へ戻す。|
|GitHub API / CLI 一時失敗|retry または `hold`。|
|merge conflict|Worker Agent に解消を依頼し、`review_fixing` へ戻す。|
|canonical repo の `master` pull 失敗|PR 状態を再取得し、task source file 反映と復旧判断を行う。|
|task source file の更新 conflict|Orchestrator Agent が再読込し、ユーザー入力または再計画を優先する。|

---

## 15. `gh-review-hook` 要件

### 15.1 基本方針

`gh-review-hook` は Worker Agent の実行ループの一部とする。

Worker Agent は PR 作成後、`gh-review-hook` を自律的に実行し、指摘がなくなるまで修正、commit、push、再実行を繰り返す。

Orchestrator Agent は `gh-review-hook` の結果を監視できるが、通常はその都度の修正判断へ介入しない。

*※ MVP では、`gh-review-hook` の実行コマンドおよびタイムアウト等の動作設定は、プロジェクトメタデータ (`project.json`) の `gh_review_hook` 設定ブロックにてプロジェクトごとに構成・保持できる設計とする。*

### 15.2 実行 cwd

`gh-review-hook` は対象 WorkExecution の worktree を cwd として実行する。

Worker Agent が実行する場合も、Controller が merge 直前に再実行する場合も、対象 WorkExecution の worktree を cwd とする。

対象 worktree は merge 完了と cleanup が終わるまで保持する。

### 15.3 exit code

|exit code|意味|出力|
|---|---|---|
|0|マージ可能。Greptile、CodeRabbit AI、GitHub review、CI、required check、conflict などに blocking な問題がない。|通常出力は任意。|
|2|レビュー、CI、required check、conflict などに対応すべき問題がある。|stderr にレビューや CI の出力、conflict 情報、対応すべき内容を出す。|
|その他の非 0|hook 自身の実行失敗、timeout、認証失敗、GitHub API 失敗など。|stderr にエラー内容を出す。|

### 15.4 Worker Agent の扱い

- exit 0 の場合、PR はマージ可能と判断し、`task.md` を `merge_ready` に更新する。
- exit 2 の場合、stderr を読み、必要な修正、commit、push、再実行を行う。
- その他の非 0 の場合、一時的な hook 失敗として retry できる。繰り返し失敗する場合は `task.md` に blocker として記録し、Orchestrator Agent に通知される。

---

## 16. Hook / lifecycle / 完了検知

### 16.1 基本方針

Agent は自分で shell / cmux tab を終了できることを前提としない。

WorkExecution の進捗・完了検知は、以下の組み合わせで行う。

- `task.md` の更新。
- file watcher event。
- Agent 実装の lifecycle hook / exit hook。
- heartbeat。
- PR 状態。
- `gh-review-hook` 結果。

Controller は shell process の終了だけを WorkExecution 完了条件として扱ってはならない。

### 16.2 lifecycle hook

Controller は Agent 実装固有の終了・停止・応答完了イベントを、以下の抽象イベントへ写像する。

```text
agent_lifecycle_stop
```

`agent_lifecycle_stop` は shell process の終了とは限らない。

`agent_lifecycle_stop` 発火時、Controller は次を行う。

1. 対象 AgentSession / WorkExecution を特定する。
2. `task.md` の存在と非空を確認する。
3. event を JSONL に記録する。
5. Orchestrator Agent に通知する。

hook は成果物を生成してはならない。

### 16.3 `task.md` の検証

MVP における主要成果物は WorkExecution の `task.md` とする。

Controller は file watcher または lifecycle hook 発火時に、`task.md` の存在と非空を確認する。

Controller は `task.md` の front matter や本文の意味内容を厳密検証しない。

---

## 17. Control Interface

### 17.1 基本方針

MVP における Orchestrator Control Interface は CLI を主経路とする。

MCP は MVP では必須とせず、将来拡張または任意 integration とする。

Orchestrator Agent は Controller CLI により状況取得、AgentSession 作成、prompt 投入、状態取得、merge execute、cleanup などを行う。

Worker Agent への実作業指示は、原則として prompt 投入と個別 task file により行う。

Controller は Orchestrator Agent の自然文出力を状態変更の正本として扱わない。状態変更は、CLI 呼び出し、file watcher event、hook 結果、プロセス監視、PR 状態取得などの明示的イベントに基づいて記録する。

### 17.2 概念操作

詳細な CLI 名・引数は operational flow 確定後に定義する。MVP で必要な概念操作は以下とする。

|操作|目的|
|---|---|
|`project status`|Project、WorkExecution、AgentSession、PR、merge lock の要約取得。|
|`work create`|work_item_ref から WorkExecution、task.md、branch、worktree を作成。|
|`agent attach`|WorkExecution に AgentSession を writer / reviewer / observer / diagnostic として attach。|
|`agent prompt`|AgentSession に prompt を投入。|
|`agent stop`|AgentSession / cmux tab / tmux session を停止。|
|`lease acquire`|write lease を取得。|
|`lease release`|write lease を解放。|
|`merge execute`|merge_lock、PR head 確認、必要な hook 再実行、`gh pr merge --merge`、`master` pull、state 更新を実行。|
|`cleanup work`|merged / canceled / superseded WorkExecution の AgentSession、tabs、worktree を cleanup。|
|`recovery digest`|復旧判断に必要な現在状況を取得。|

状態取得系操作は `--json` 出力を提供することが望ましい。

状態変更系操作は、成功時に更新対象 resource と `event_id` を返すことが望ましい。

---

## 18. 状態モデル

### 18.1 基本方針

Controller は業務上の Task state machine を正本として持たない。

Controller が保持するのは execution state である。

execution state は、Project、WorkExecution、AgentSession、Artifact、PR、write lease、merge lock、file watcher event を中心に構成する。

### 18.2 WorkExecution state

|state|意味|
|---|---|
|`created`|WorkExecution が作成された。|
|`task_file_created`|個別 `task.md` が作成された。|
|`dispatched`|AgentSession が attach された。|
|`running`|writer AgentSession が作業中。|
|`pr_open`|PR が作成された。|
|`gate_check`|`gh-review-hook` 実行中または再実行待ち。|
|`review_fixing`|レビュー指摘や `gh-review-hook` exit 2 に対応して修正中。|
|`merge_ready`|Worker Agent が PR をマージ可能と判断した。|
|`merging`|Orchestrator Agent 責任で Controller CLI がマージ処理中。|
|`merged`|PR が merge 完了した状態。マージ後の canonical repo 同期（sync）の成否・保留状態は、`sync_status`（`pending | success | aborted`）で表す。|
|`followup_required`|残作業や追加 Task の反映が必要。|
|`returned`|Worker Agent が作業せず差し戻した。|
|`blocked`|外部要因、依存関係、判断待ち。|
|`failed`|WorkExecution が異常状態。|
|`hold`|自動処理を停止し、ユーザーまたは Orchestrator Agent の明示判断待ち。|
|`canceled`|通常中止。|
|`superseded`|分割や再計画により別 WorkExecution へ置き換えられた。|

### 18.3 AgentSession state

|state|意味|
|---|---|
|`starting`|cmux tab / tmux session / Harness 起動中。|
|`running`|AgentSession が稼働中。|
|`idle`|入力待ちまたは明示待機中。|
|`hung`|heartbeat 停止等によりハング候補。|
|`stopping`|停止処理中。|
|`exited`|AgentSession が終了した。|
|`lost`|process / tab / session が消失した。|
|`detached`|WorkExecution から detach された。|

### 18.4 WriteLease state

|state|意味|
|---|---|
|`active`|writer AgentSession が有効な write lease を持つ。|
|`stale`|heartbeat 停止、process 消失等により lease が古い可能性がある。|
|`revoked`|Orchestrator Agent 判断により取り消された。|
|`released`|正常に解放された。|

### 18.5 Artifact state

|state|意味|
|---|---|
|`pending`|まだ検証されていない。|
|`ready`|`task.md` または補助成果物が存在し、非空である。|
|`invalid`|存在しない、または空である。|

### 18.6 基本遷移

|From|Event / 条件|To|主担当|
|---|---|---|---|
|`created`|`task.md` 作成|`task_file_created`|Orchestrator Agent / Controller|
|`task_file_created`|writer AgentSession attach|`dispatched`|Orchestrator Agent / Controller|
|`dispatched`|AgentSession 起動|`running`|Controller|
|`running`|PR 作成|`pr_open`|Worker Agent|
|`pr_open`|`gh-review-hook` 実行|`gate_check`|Worker Agent|
|`gate_check`|`gh-review-hook` exit 2|`review_fixing`|Worker Agent|
|`review_fixing`|修正後再実行|`gate_check`|Worker Agent|
|`gate_check`|`gh-review-hook` exit 0|`merge_ready`|Worker Agent|
|`merge_ready`|Orchestrator Agent が merge execute 開始|`merging`|Orchestrator Agent / Controller|
|`merging`|merge 成功、task source file 反映完了|`merged`|Orchestrator Agent / Controller|
|`merging`|PR head 変化、hook exit 2、conflict|`gate_check` または `review_fixing`|Orchestrator Agent|
|`running`|作業前差し戻し|`returned`|Worker Agent|
|`returned`|分割・再計画|`superseded` または `task_file_created`|Orchestrator Agent|
|`running`|ハング、異常、検証失敗|`failed` または `hold`|Controller / Orchestrator Agent|
|任意|Circuit Breaker|`hold`|Controller / Orchestrator Agent|
|`hold`|resume|`running` または `dispatched`|Orchestrator Agent|
|`hold`|cancel|`canceled`|Orchestrator Agent|

---

## 19. Circuit Breaker

同一 WorkExecution または同一 work_item_ref に対する連続 retry には上限回数を設ける。

既定では `max_consecutive_retries` を設定値として持ち、上限到達時は `hold` 候補とする。

同一 WorkExecution には総実行時間上限 `max_work_execution_runtime` も設定でき、これを超過した場合も `hold` 候補とする。

`gh-review-hook` exit 2 が同一内容で一定回数以上繰り返される場合、または一定時間以上 `review_fixing` / `gate_check` を往復する場合、Circuit Breaker 候補とする。

`hold` へ遷移した WorkExecution は、ユーザーまたは Orchestrator Agent の明示判断なしに自動再開してはならない。

Circuit Breaker 発火時は、直前失敗理由、直近 diff、最終コマンド、review 状態、`gh-review-hook` stderr、`task.md`、PR 状態、write lease 状態を保存し、再開判断材料として残す。

---

## 20. 人間入力の優先順位

Controller は入力イベントを以下のように区別して保存する。

|入力元|分類|優先度|
|---|---|---|
|ユーザーが直接入力|`user_intervention`|最高|
|Orchestrator Agent の指示|`orchestrator_instruction`|中|
|Worker Agent の自律判断|`worker_update`|低|
|Controller の機械的イベント|`controller_event`|状態管理用|
|file watcher event|`file_watcher_event`|状態管理用|

ユーザー入力は、過去の Orchestrator Agent の計画や Worker Agent の自律判断より優先される。

Controller はユーザー入力と Agent 自律出力を区別して記録・表示する。

---

## 21. 永続化設計

重要イベントは JSONL と SQLite の両方に保存する。

- **JSONL** は append-only の監査ログであり、時系列の正本とする。
- **SQLite** は UI 表示、検索、集計、復旧のための execution state view とする。

SQLite は JSONL から再構築可能でなければならない。

各イベントは、少なくとも以下の情報を持つ。

- `event_id`
- `timestamp`
- `project_id`
- `work_execution_id` (optional: 特定の WorkExecution に紐づかない Project 全体 event では null を許容する)
- `agent_session_id` (optional: 特定の AgentSession に紐づかない event では null を許容する)
- `actor`: `user|controller|orchestrator_agent|worker_agent|hook|file_watcher`
- `event_type`
- `summary`
- `payload`

`payload` には必要に応じて以下を含める。

- `work_item_ref`
- `task_file_path`
- `task_source_file_path`
- `branch`
- `worktree_path`
- PR 情報
- artifact 状態
- write lease 情報
- merge lock 情報
- cmux tab 情報
- tmux session 情報

状態遷移は、JSONL への event append が成功した後に SQLite へ反映する。

SQLite 更新に失敗した場合、JSONL から再構築できる。

JSONL 書き込みに失敗した場合、その状態遷移は確定してはならない。

MVP では Project 単位で file lock を取得して JSONL に append する。

---

## 22. プロセス監視と復旧

Controller は各 AgentSession に対して以下を管理する。

- `agent_session_id`
- role
- attach mode
- cmux tab id
- tmux session id
- PID
- cwd
- 開始時刻
- 最終 heartbeat
- lifecycle hook event
- 終了コード
- write lease の有無

heartbeat は PTY 入出力、hook 発火、file watcher event、明示的 keepalive event のいずれかで更新できる。

一定時間 heartbeat が更新されず、かつ process 存在確認に失敗した場合、その AgentSession は `lost` または `hung` 候補とする。

writer AgentSession が `hung` または `lost` の場合、Controller は write lease を stale として扱い、Orchestrator Agent に通知する。

Orchestrator Agent は recovery digest を読み、必要に応じて以下を判断する。

- 既存 AgentSession へ状況確認 prompt を投入する。
- 古い writer AgentSession を stop / detach する。
- 新しい AgentSession を同一 WorkExecution に attach する。
- write lease を移譲する。
- WorkExecution を hold する。
- WorkExecution を split / supersede する。
- merge_ready / merging 状態を再評価する。

再起動後、孤立した `running` 状態を残してはならない。

### 22.1 recovery digest

Controller は復旧時、少なくとも以下を収集する。

- SQLite 上の Project / WorkExecution / AgentSession / PR / artifact / merge lock / write lease 状態。
- JSONL 監査ログの最新 event。
- task source file の存在、hash、変更時刻。
- 各 WorkExecution の `task.md` の存在、hash、front matter。
- repo 外 worktree の存在、branch、未 commit diff。
- write lease の状態。
- remote branch の存在。
- PR の存在、状態、review 状態、merge conflict 状態。
- merge lock の状態。
- canonical repo の `master` 状態と dirty state。
- 既知の PID の生存確認。
- 直近 heartbeat、終了コード、timeout、強制終了の有無。
- 各 WorkExecution に紐づく `work_item_ref`。
- cmux tab / tmux session の存在。

---

## 23. Cleanup 要件

merge 完了後、Orchestrator Agent が task source file 反映と follow-up 判断を終えた場合、Controller は当該 WorkExecution に紐づく AgentSession / cmux tab / tmux session / worktree を cleanup できる。

基本方針として、merge 済み WorkExecution の worktree と Worker AgentSession は閉じる。

cleanup 前に Controller は以下を確認する。

- PR が merged である。
- `task.md` が保存済みである。
- write lease が released / revoked である。
- merge lock が released である。
- Orchestrator Agent が follow-up を task source file へ反映済み、または不要と判断済みである。

cleanup 対象は以下とする。

- WorkExecution に紐づく Worker / Reviewer / Diagnostic AgentSession
- cmux tabs
- tmux sessions
- stale write lease
- worktree

cleanup 前に、Controller は以下を JSONL に記録する。

- branch
- PR number
- head commit
- merge commit
- task file path
- changed files
- cleanup time

必要に応じて、merged worktree は一定期間保持できる設定を持ってよい。

---

## 24. 受け入れ基準

- Controller / Orchestrator Agent / Worker Agent の責務が実装上分離されている。
- 1 Project が 1 cmux workspace に対応する。
- 1 AgentSession が 1 cmux tab と 1 tmux session に対応する。
- 作業管理の業務上の正本は、ユーザーが指定した task source file である。
- Controller は業務上の Task state machine を正本として持たない。
- Orchestrator Agent は task source file を読み、必要に応じて更新できる。
- Worker Agent は task source file を編集しない。
- Orchestrator Agent は WorkExecution ごとに個別 `task.md` を作成できる。
- Worker Agent は `task.md` を読み、進捗・PR・レビュー結果・残作業・差し戻し理由を同ファイルへ更新できる。
- Controller は task source file の変更を検知し、Orchestrator Agent に通知できる。
- Controller は `task.md` の変更を検知し、Orchestrator Agent に通知できる。
- 1 WorkExecution が 1 branch / 1 worktree / 0..1 PR / 1 `task.md` を持つ。
- 1 WorkExecution に複数 AgentSession を attach できる。
- 同一 worktree の active writer は最大 1 AgentSession に制限される。
- reviewer / observer / diagnostic AgentSession は read-only attachment として複数同時に attach できる。
- Writer AgentSession がハングしても、Orchestrator Agent の判断で別 AgentSession が同じ worktree を引き継げる。
- 複数 WorkExecution が同一リポジトリで並行しても、repo 外 worktree により作業領域が分離される。
- Worker Agent は 1 WorkExecution に対し、実装、レビュー、PR、`gh-review-hook` 反復、`merge_ready` 報告までを実行できる。
- Worker Agent は `gh-review-hook` exit 2 の stderr を読み、レビュー・CI・conflict 指摘へ対応できる。
- Worker Agent は `gh pr merge` や `master` checkout / pull を行わない。
- `gh-review-hook` は対象 WorkExecution の worktree を cwd として実行される。
- Orchestrator Agent は Worker Agent の `merge_ready` 報告後、マージするか、差し戻すか、hold するかを判断できる。
- マージ処理は Orchestrator Agent が責任主体となり、Controller CLI が merge lock、PR head 確認、`gh pr merge --merge`、`master` pull、state 更新を機械的に実行できる。
- マージ処理後、canonical repo の `master` が pull され、必要に応じて task source file が更新される。
- Worker Agent は作業せずに差し戻せる。この場合、未コミット差分は廃棄できる。
- Worker Agent は残作業を `task.md` に記載し、Orchestrator Agent へ返せる。
- Agent は自分で shell / cmux tab を終了できることを前提とせず、file watcher と lifecycle hook により完了・更新を検知できる。
- アプリ再起動時、Controller が状況を収集し、Orchestrator Agent が CLI とプロンプト投入により復旧方針を実行できる。
- JSONL は監査ログの正本、SQLite は execution state view として機能する。
- ID は ULID を基本とする。

---

## 25. 将来拡張

- MCP tool 化。
- `base_branch` 設定化。
- task source file の Markdown profile、front matter、独自 comment anchor の標準化。
- Orchestrator Agent / Worker Agent 実装の差し替え。
- 複数プロジェクト、複数 cmux workspace、分散 Worker Agent。
- Windows / WSL 対応。
- worktree cleanup policy の詳細設定。
- reviewer AgentSession の自動起動。
- PR / issue tracker / external task system 連携。

---

## 26. 採用方針まとめ

- UI / workspace 基盤は cmux に寄せる。
- 1 Project は 1 cmux workspace とする。
- 1 AgentSession は 1 cmux tab と 1 tmux session に対応する。
- Controller の中核ロジックは Rust で実装する。
- 作業管理の業務上の正本は task source file とする。
- Controller は業務上の Task 台帳を正本として持たず、execution state のみを管理する。
- Orchestrator Agent は task source file の確認、更新、WorkExecution 作成、AgentSession 制御、復旧判断、マージ責任を担う。
- Worker Agent は 1 WorkExecution を実装開始から `merge_ready` 報告まで担当する。
- Worker Agent は task source file を編集しない。
- Orchestrator Agent は WorkExecution ごとに個別 `task.md` を作成し、Worker Agent に渡す。
- Worker Agent は `task.md` を読み、作業結果・PR・hook 結果・残作業を同ファイルに追記する。
- Controller は task source file と `task.md` の変更を file watcher で検知し、Orchestrator Agent に通知する。
- worktree と Worker AgentSession は 1:1 固定にしない。
- 1 worktree / branch / PR / task.md に対して複数 AgentSession を紐づけられる。
- ただし同一 worktree の active writer は最大 1 AgentSession とする。
- reviewer / observer / diagnostic AgentSession は read-only attachment として複数許可する。
- AgentSession のハングは WorkExecution の即時失敗を意味しない。
- Worker Agent は `gh-review-hook` exit 0 によりマージ可能判断を行う。
- マージ責任主体は Orchestrator Agent とする。
- 実際の merge command、merge lock、PR head 確認、`master` pull、execution state 更新は Rust Controller CLI が実行してよい。
- `gh-review-hook` は対象 WorkExecution の worktree を cwd として実行する。
- Agent は自力で shell / cmux tab を終了できることを前提としない。
- file watcher と Agent lifecycle hook により WorkExecution の進捗、完了、差し戻しを検知する。
- merge 後、Controller は Worker / Reviewer / Diagnostic AgentSession、cmux tab、tmux session、worktree を cleanup できる。
- ID は ULID を基本とする。
- project_id は初回登録時に生成する ULID とし、path slug は表示用に分離する。
- JSONL は監査ログの正本、SQLite は execution state view とする。
- 同一リポジトリ前提とする。
