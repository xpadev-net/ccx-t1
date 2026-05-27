# CCX Orchestrator 超詳細実装設計 & タスク台帳 (改訂 v3.1 / P0・P1 最終整合版)

本ドキュメントは、`ccx_requirements_parallelized_v3.md` の要件に完全準拠し、最新の **「cmux 中心 / 1 worktree : N AgentSession / task.md 主成果物 / 競合防止ルール」** の設計方針を反映した CCX Controller (Rust) の実装設計書兼タスク台帳である。

---

## 📂 0. リポジトリ構成 (Monorepo Layout)

開発効率の最大化、バージョン不一致（Socket API や SQLite スキーマ変更時）の防止、およびビルド・配布パッケージの一元化のため、CCX は**単一の Monorepo (モノレポ)** として構成する。

```text
ccx/ (Root)
├── Cargo.toml              # Rust コントローラーのワークスペース設定
├── src/                    # Rust コントローラー (CLI & 制御コア)
│   ├── main.rs
│   └── ...
├── gui/                    # ccx-cmux GUI アプリケーション (Swift/AppKit/libghostty)
│   │                       # ※ cmux 本家からフォークし、subtree 又は直接配置
│   ├── Package.swift
│   └── ...
├── z/
│   ├── tasks.md            # 本タスク台帳・実装設計書
│   └── ...
└── ccx_requirements_parallelized_v3.md
```

---

## 📂 1. システムモジュール構成 (Rust Module Architecture)

概念上の主役である `cmux` を上位とし、`tmux` を内部のプロセスアダプターとして位置づけるモジュール構成に刷新する。また、状態遷移の整合性を担保する `transition`、および JSONL から SQLite への書き出しを担う `projector` を明示する。

```text
src/
├── main.rs                 # エントリーポイント。引数パースと各サブコマンドへのディスパッチ。
├── cli/                    # CLI コマンドハンドラー (Clap サブコマンド実装)。
├──   ├── mod.rs
├──   ├── project.rs        # register, list, open, status
├──   ├── work.rs           # create, cleanup, merge_execute
├──   ├── agent.rs          # start-orchestrator, attach, prompt, stop
├──   └── lease.rs          # acquire, release
├── config/                 # CCX_HOME や project.json (~/.ccx/projects/<id>/project.json) の管理。
│   ├── mod.rs
│   └── project_config.rs   # gh_review_hook や cleanup_policy 設定値。
├── error.rs                # 共通エラー型 (CcxError) の定義 (thiserror クレート of 活用)。
├── domain/                 # ドメインモデル、エンティティ、状態 Enum 定義。
│   ├── mod.rs
│   ├── project.rs          # Project / ProjectMetadata
│   ├── work_execution.rs   # WorkExecution / WorkExecutionState
│   ├── agent_session.rs    # AgentSession / AgentSessionState
│   ├── lease.rs            # WriteLease / WriteLeaseState
│   ├── merge_lock.rs       # MergeLock / MergeLockState
│   ├── event.rs            # Event / Actor / EventType の完全定義
│   └── transition.rs       # 状態遷移 validator (validate_transition)
├── persistence/            # 永続化層。JSONL 監査ログおよび SQLite 制御。
│   ├── mod.rs
│   ├── jsonl.rs            # append-only JSONL 書込、fd-lock によるファイルロック。
│   ├── sqlite.rs           # SQLite 接続、マイグレーション、dirty フラグ管理。
│   ├── projector.rs        # イベントを受信して SQLite 状態ビューを更新する射影器。
│   └── rebuild.rs          # SQLite 再構築 (rebuild) および整合性検証 (verify)。
├── git/                    # Git worktree, branch, PR, canonical repo 操作。
│   ├── mod.rs
│   ├── repo.rs             # canonical repo 操作 (dirtyチェック, master pull, sync)。
│   ├── worktree.rs         # Git worktree の作成・削除・クリーンアップ。
│   ├── github.rs           # gh CLI を介した PR 作成・状態確認・マージ実行。
│   └── review_hook.rs      # gh-review-hook の worktree 内実行と exit code 判定。
├── agent_runtime/          # cmux tab UI および tmux プロセス実行基盤。
│   ├── mod.rs
│   ├── cmux_adapter.rs     # cmux tab 作成、workspace 管理 (Trait 抽象化)。
│   ├── tmux_adapter.rs     # 内部プロセス実行詳細。tmux session 新規作成、停止。
│   ├── harness.rs          # 実行プロセス (PTY) 管理、PID/cwd 監視、heartbeat 追跡。
│   ├── prompt.rs           # prompt 注入処理 (file / stdin 経由)。
│   └── transcript.rs       # アフター監査用の入出力ログ (transcript.log) 出力。
├── watcher/                # ファイル監視モジュール。
│   ├── mod.rs
│   ├── source_watcher.rs   # task source file の監視とイベント発火。
│   └── task_watcher.rs     # 各 work-execution 内の task.md 監視、front matter 解析およびイベント発行。
├── recovery/               # recovery digest 生成と lost セッション等の検出。
│   ├── mod.rs
│   └── digest.rs
└── breaker/                # Circuit Breaker 監視と hold 遷移制御。
    ├── mod.rs
    └── evaluator.rs
```

---

## 🗄️ 2. データモデル & 永続化スキーマ詳細

### 2.1 状態管理とファイルの役割分担

| 永続化対象 | 所有・適用単位 | 役割・正本パス |
| :--- | :--- | :--- |
| **`task.md`** | WorkExecution | **【主要成果物・状態正本】** `~/.ccx/projects/<project_id>/work-executions/<work_execution_id>/task.md` を正本パスに固定。 (※ worktree 側に `.ccx-task.md` の名前で symlink を置いてもよいが、正本は管理ディレクトリ側とする) |
| **`events.jsonl`** | Project | **【監査ログ正本】** 時系列のすべての状態変化・操作イベントを追記するアペンデントオンリーログ。 |
| **`state.sqlite`** | Project | **【状態ビュー】** JSONL からいつでも完全再構築可能な、UI・CLI用のリードモデル。 |

> [!NOTE]
> MVP における read-only attachment (reviewer / observer / diagnostic) は、Controller の write lease と prompt / 運用規約による論理的制約であり、OS ファイル権限による完全な書き込み禁止は必須としない（ポリシー上の制約として扱う）。

### 2.2 task.md status と WorkExecutionState の対応

Controller は `task.md` の front matter `status` を補助メタデータとして読み取り、状態を決定する。対応は以下とする。

| task.md status | WorkExecutionState | 役割・説明 |
|---|---|---|
| `assigned` | `task_file_created` | Worker Agent が割り当てられた初期状態 |
| `working` | `running` | Worker Agent が作業実行中の状態 |
| `pr_open` | `pr_open` | PR がオープンされた状態 |
| `gate_check` | `gate_check` | 自動 gate hook 実行中または完了後の検証状態 |
| `review_fixing` | `review_fixing` | レビュー指摘の修正中状態 |
| `merge_ready` | `merge_ready` | マージ準備完了状態（Orchestrator の承認・merge 待ち） |
| `returned` | `returned` | 指摘やエラーによる差し戻し状態 |
| `blocked` | `blocked` | 外部要因などでブロックされた状態 |
| `failed` | `failed` | 失敗状態 |
| `followup_required` | `followup_required` | マージ成功後に follow-up が必要な状態 |
| `merged` | `merged` | 正常にマージ完了した状態 |

※ `created`, `dispatched`, `merging`, `hold`, `canceled`, `superseded` は Controller / Orchestrator 操作により更新され、Worker Agent が `task.md` から直接設定する通常 status ではない。

### 2.3 task.md 更新ルール (同時編集競合対策)

`task.md` は WorkExecution の共有作業ファイルであるため、同時編集による不整合や競合を防ぐため以下の役割・所有ルールを適用する。

| 領域 | 更新できる主体 |
|---|---|
| **front matter** | Controller / Orchestrator / active writer のみ。reviewer / diagnostic は原則更新禁止。 |
| **Progress** | active writer のみ更新可能。 |
| **Review / Gate** | active writer または reviewer。reviewer は session-specific subheading に追記。 |
| **Result** | active writer のみ更新可能。 |
| **Remaining Work** | active writer / Orchestrator のみ。 |
| **Blockers** | active writer / Orchestrator / diagnostic のみ。 |

**競合検知ルール**:
複数 AgentSession が同一 `task.md` を直接編集する場合、Controller は file hash または mtime により変更競合を検知し、Orchestrator Agent に警告・通知する。

### 2.4 SQLite スキーマ詳細 (state.sqlite DDL)

制約（CHECK 制約、排他的な UNIQUE 部分インデックス、および Artifact状態カラム）を追加し、整合性を担保する。

```sql
PRAGMA foreign_keys = ON;

-- プロジェクトテーブル
CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT PRIMARY KEY,
    display_slug TEXT NOT NULL,
    canonical_repo TEXT NOT NULL,
    task_source_file TEXT NOT NULL,
    sqlite_dirty INTEGER NOT NULL DEFAULT 0, -- 1 の場合は再構築が必要
    created_at TEXT NOT NULL
);

-- WorkExecutionテーブル
CREATE TABLE IF NOT EXISTS work_executions (
    work_execution_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN (
        'created', 'task_file_created', 'dispatched', 'running', 'pr_open', 
        'gate_check', 'review_fixing', 'merge_ready', 'merging', 'merged', 
        'followup_required', 'returned', 'blocked', 'failed', 'hold', 'canceled', 'superseded'
    )),
    branch_name TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    task_file_path TEXT NOT NULL, -- work-executions/<id>/task.md を指す
    pr_number INTEGER,
    pr_url TEXT,
    head_commit TEXT,
    source_path TEXT NOT NULL,
    selector_type TEXT NOT NULL,
    selector_value TEXT NOT NULL,
    display_text TEXT NOT NULL,
    source_file_hash TEXT NOT NULL,
    selected_at TEXT NOT NULL,
    artifact_state TEXT NOT NULL DEFAULT 'pending' CHECK(artifact_state IN ('pending', 'ready', 'invalid')),
    artifact_checked_at TEXT,
    sync_status TEXT NOT NULL DEFAULT 'pending' CHECK(sync_status IN ('pending', 'success', 'aborted')),
    sync_warning TEXT,
    FOREIGN KEY(project_id) REFERENCES projects(project_id)
);

-- AgentSessionテーブル
CREATE TABLE IF NOT EXISTS agent_sessions (
    agent_session_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    work_execution_id TEXT, -- orchestrator の場合は NULL
    state TEXT NOT NULL CHECK(state IN (
        'starting', 'running', 'idle', 'hung', 'stopping', 'exited', 'lost', 'detached'
    )),
    role TEXT NOT NULL CHECK(role IN ('orchestrator', 'worker', 'reviewer', 'diagnostic')),
    attach_mode TEXT CHECK(attach_mode IS NULL OR attach_mode IN ('writer', 'reviewer', 'observer', 'diagnostic')),
    cmux_tab_id TEXT NOT NULL,
    tmux_session_id TEXT NOT NULL,
    pid INTEGER,
    cwd TEXT NOT NULL,
    started_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL,
    exit_code INTEGER,
    FOREIGN KEY(project_id) REFERENCES projects(project_id),
    FOREIGN KEY(work_execution_id) REFERENCES work_executions(work_execution_id),
    CHECK(
        (role = 'orchestrator' AND work_execution_id IS NULL AND attach_mode IS NULL)
        OR
        (role <> 'orchestrator' AND work_execution_id IS NOT NULL AND attach_mode IS NOT NULL)
    )
);

-- プロジェクトごとに active な Orchestrator は最大 1 つとする部分インデックス
CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_orchestrator_per_project
ON agent_sessions(project_id)
WHERE role = 'orchestrator'
  AND state IN ('starting', 'running', 'idle');

-- WriteLeaseテーブル (同一 worktree / WorkExecution に対する active writer を最大 1 つに制限)
CREATE TABLE IF NOT EXISTS write_leases (
    write_lease_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    work_execution_id TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    writer_agent_session_id TEXT NOT NULL,
    acquired_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active', 'stale', 'released', 'revoked')),
    FOREIGN KEY(project_id) REFERENCES projects(project_id),
    FOREIGN KEY(work_execution_id) REFERENCES work_executions(work_execution_id),
    FOREIGN KEY(writer_agent_session_id) REFERENCES agent_sessions(agent_session_id)
);

-- 1つの WorkExecution において、active な write lease は同時に最大 1 つとする部分インデックス
CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_write_lease_per_work_execution
ON write_leases(work_execution_id)
WHERE state = 'active';

-- MergeLockテーブル (プロジェクト単位で同時に 1 つだけマージを実行可能に制御)
CREATE TABLE IF NOT EXISTS merge_locks (
    merge_lock_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    owner_agent_session_id TEXT NOT NULL,
    work_execution_id TEXT NOT NULL,
    pr_number INTEGER NOT NULL,
    acquired_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active', 'stale', 'released')),
    FOREIGN KEY(project_id) REFERENCES projects(project_id),
    FOREIGN KEY(owner_agent_session_id) REFERENCES agent_sessions(agent_session_id),
    FOREIGN KEY(work_execution_id) REFERENCES work_executions(work_execution_id)
);

-- 1つのプロジェクトにおいて、active な merge lock は同時に最大 1 つとする部分インデックス
CREATE UNIQUE INDEX IF NOT EXISTS idx_one_active_merge_lock_per_project
ON merge_locks(project_id)
WHERE state = 'active';
```

---

## 🔄 3. 状態遷移 Validator & EventType 定義

### 3.1 EventType の完全定義 (`src/domain/event.rs`)

SQLite の再構築処理（Projector）や状態遷移評価で利用するイベント型の一覧をあらかじめ確定させる。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    ProjectRegistered,
    TaskSourceFileChanged,
    WorkExecutionCreated,
    WorkExecutionTaskFileCreated,
    WorkExecutionStateChanged,
    WorkExecutionTaskFileChanged,
    AgentSessionCreated,
    AgentSessionAttached,
    AgentSessionPrompted,
    AgentSessionHeartbeat,
    AgentSessionHung,
    AgentSessionStopped,
    AgentLifecycleStop,
    WriteLeaseAcquired,
    WriteLeaseReleased,
    WriteLeaseStale,
    WriteLeaseRevoked,
    PrOpened,
    PrHeadUpdated,
    GhReviewHookStarted,
    GhReviewHookCompleted,
    MergeLockAcquired,
    MergeStarted,
    MergeCompleted,
    MergeFailed,
    CanonicalSyncCompleted,
    CanonicalSyncFailed,
    CleanupStarted,
    CleanupCompleted,
    UserIntervention,
    WorktreeCreated,
    BranchCreated,
}
```

### 3.2 状態遷移 Validator (`src/domain/transition.rs`)

CLI やファイルウォッチャーが不正な状態遷移を直接書き込むのを防ぐため、以下の許可ルールに基づく検証を行う。

```rust
use crate::domain::work_execution::WorkExecutionState;
use crate::error::CcxError;

pub fn validate_transition(
    from: WorkExecutionState,
    to: WorkExecutionState,
) -> Result<(), CcxError> {
    if from == to {
        return Ok(());
    }
    
    let is_valid = match (from, to) {
        // 正常フロー
        (WorkExecutionState::Created, WorkExecutionState::TaskFileCreated) => true,
        (WorkExecutionState::TaskFileCreated, WorkExecutionState::Dispatched) => true,
        (WorkExecutionState::Dispatched, WorkExecutionState::Running) => true,
        (WorkExecutionState::Running, WorkExecutionState::PrOpen) => true,
        (WorkExecutionState::PrOpen, WorkExecutionState::GateCheck) => true,
        (WorkExecutionState::GateCheck, WorkExecutionState::ReviewFixing) => true,
        (WorkExecutionState::ReviewFixing, WorkExecutionState::GateCheck) => true,
        (WorkExecutionState::GateCheck, WorkExecutionState::MergeReady) => true,
        (WorkExecutionState::MergeReady, WorkExecutionState::Merging) => true,
        (WorkExecutionState::Merging, WorkExecutionState::Merged) => true,
        
        // 差し戻しフロー
        (WorkExecutionState::Running, WorkExecutionState::Returned) => true,
        (WorkExecutionState::Returned, WorkExecutionState::Hold) => true,
        (WorkExecutionState::Returned, WorkExecutionState::Superseded) => true,
        (WorkExecutionState::Returned, WorkExecutionState::TaskFileCreated) => true,
        
        // 中断・失敗・ブロック
        (WorkExecutionState::Running, WorkExecutionState::Blocked) => true,
        (WorkExecutionState::Running, WorkExecutionState::Failed) => true,
        (WorkExecutionState::GateCheck, WorkExecutionState::Failed) => true,
        (WorkExecutionState::Merging, WorkExecutionState::GateCheck) => true, // SHA不一致等のマージ差し戻し
        (WorkExecutionState::Merging, WorkExecutionState::ReviewFixing) => true,
        
        // 任意の状態で Hold や Canceled へ遷移可能
        (_, WorkExecutionState::Hold) => true,
        (_, WorkExecutionState::Canceled) => true,

        // retry / resume 系統遷移 (P1-5 追加分)
        (WorkExecutionState::Failed, WorkExecutionState::Dispatched) => true,
        (WorkExecutionState::Failed, WorkExecutionState::TaskFileCreated) => true,
        (WorkExecutionState::Blocked, WorkExecutionState::Running) => true,
        (WorkExecutionState::Blocked, WorkExecutionState::Hold) => true,
        (WorkExecutionState::Hold, WorkExecutionState::Dispatched) => true,
        (WorkExecutionState::Hold, WorkExecutionState::Running) => true,
        (WorkExecutionState::Hold, WorkExecutionState::TaskFileCreated) => true,
        (WorkExecutionState::MergeReady, WorkExecutionState::Hold) => true,
        (WorkExecutionState::GateCheck, WorkExecutionState::Blocked) => true,
        (WorkExecutionState::ReviewFixing, WorkExecutionState::Blocked) => true,
        
        _ => false,
    };

    if is_valid {
        Ok(())
    } else {
        Err(CcxError::InvalidStateTransition { from, to })
    }
}
```

---

## 🔄 4. 主要処理フロー & 安全設計

### 4.1 task_watcher 変更検知イベント発火フロー

`task_watcher` は `task.md` の変更を検知した際、SQLite を直接更新せず、JSONL 監査ログを経由して安全に反映する。また、ノイズ抑制のための重複検知処理を挟む。

```text
[task.md modified (by Worker/etc)]
          │
          ▼
1. Detect file modification (via notify crate)
          │
          ▼
2. Hash Check (Deduplication)
   - Calculate SHA-256 of task.md content
   - Compare with memory cache `last_seen_hash`
   - If identical, SKIP subsequent processing (No Event, No DB Update)
          │
          ▼
3. Parse YAML Front Matter (best effort)
          │
          ▼
4. Append `work_execution_task_file_changed` event to events.jsonl
          │
          ▼
5. Projector detects JSONL event & Updates state.sqlite
          │
          ▼
6. Notify Orchestrator Agent (with priority / status change check)
```

### 4.2 SQLite Projection と遅延 Rebuild アルゴリズム

状態変更中に SQLite トランザクションが失敗した場合、処理をブロックせず、安全に遅延再構築を行う。

```text
[State Change Triggered]
          │
          ▼
1. Acquire File Lock on events.jsonl
          │
          ▼
2. Write Event to events.jsonl & fsync
          │
          ▼
3. Try to execute SQLite write transaction
          ├─────────────────────────────────────────┐
          │ (Success)                               │ (Fail)
          ▼                                         ▼
4. State projection applied                 4. - Create marker file: `state/sqlite.dirty`
          │                                    - Set `sqlite_dirty = 1` (if DB writable)
          │                                         │
          ▼                                         ▼
5. Release File Lock                        5. Return success warning ("SQLite view is dirty")
                                                    │
                                                    ▼
                                            6. Trigger reconstruction asynchronously or via CLI:
                                               `ccx db rebuild --project-id <project_id>`
                                               (removes marker file upon successful rebuild)
```

### 4.3 `merge execute` 後の安全な Canonical Repo 同期処理

ユーザーや Orchestrator の正本である task source file を破壊しないよう、マージ完了後の pull 処理を安全側に倒して処理する。

```text
[gh pr merge Successful (GitHub merged)]
          │
          ▼
1. Inside canonical repo: Run `git status --porcelain`
          ├────────────────────────┬────────────────────────┐
          │ (Clean)                │ (Dirty ONLY on         │ (Dirty on other files)
          │                        │  task source file)     │
          ▼                        ▼                        ▼
2. Run `git checkout master`   2. - Skip automatic pull    2. - Abort pull
   && `git pull --ff-only`        - State -> "merged"         - State -> "merged"
          │                       - Flag: sync_warning        - Flag: sync_warning
          │                       - Notify Orchestrator       - Notify Orchestrator
          │                         (task_source_dirty)         (hold / user_required)
          ▼                        ▼                        ▼
      [Success]             [Sync Pending]           [Sync Aborted]
```

### 4.4 cmux (Connection Multiplexer) 統合設計と CmuxAdapter Trait

cmux を UI・作業空間の正本とし、tmux をシェルプロセスの永続化レイヤー（ヘッドレス実行対応およびセッション耐障害性の担保）として位置づける協調設計を定義する。

#### 4.4.1 協調アーキテクチャ概要 (cmux × tmux & cmux フォークによる GUI 実装)

- **GUI のフォーク実装方針**: 本プロジェクトの GUI 部分は、既存の macOS ネイティブターミナルアプリケーションである **`cmux` をフォークする形 (ccx-cmux)** で実装する。これにより、一から GUI アプリを構築するコストを削減しつつ、libghostty による超高速な GPU レンダリング端末と、CCX 専用のダッシュボードを一体化した極上の統合開発体験 (IDE-like UX) を実現する。
- **永続化層 (tmux)**: AgentSession ごとに一意な tmux セッション (`ccx-<agent_session_id>`) をバックグラウンドで起動し、実シェルプロセスと Harness をここで実行する。これにより、Controller や ccx-cmux GUI の再起動、ネットワーク切断が発生しても、AI エージェントの実行状態は安全に維持される。
- **表示レイヤー (ccx-cmux)**: フォークした ccx-cmux アプリケーションに対し、Unix ドメインソケット (`/tmp/cmux.sock`) を介して指示を送り、新しい「タブ (Surface)」や「ワークスペース」を作成する。タブ内のシェルとして `tmux attach-session -t ccx-<agent_session_id>` を実行することで、美しい native macOS 端末上に tmux の出力をリアルタイムで投影し、ユーザーが直接対話可能にする。
- **ヘッドレス / CI モード (フォールバック)**: `/tmp/cmux.sock` への接続が失敗した場合（非macOS環境や ccx-cmux 未起動時）、警告ログを出力しつつ、cmux 連携処理をスキップしてバックグラウンドの tmux / Harness のみで実行を継続する。これにより、CI や CLI のみでの headless 実行を完全互換としてサポートする。

#### 4.4.2 Socket API (JSON-RPC) 仕様

Rust Controller は `/tmp/cmux.sock` に対し、以下の改行区切り (Newline-Delimited) JSON-RPC 2.0 メッセージを送信する。

1. **ワークスペース作成 (`workspace.create`)**:
   ```json
   {
     "jsonrpc": "2.0",
     "id": "ws-create-1",
     "method": "workspace.create",
     "params": {
       "name": "CCX: <project_display_slug>",
       "cwd": "<canonical_repo_path>"
     }
   }
   ```
   - **戻り値**: 新規作成された workspace ID。

2. **タブ (Surface) 作成 (`surface.create`)**:
   ```json
   {
     "jsonrpc": "2.0",
     "id": "tab-create-1",
     "method": "surface.create",
     "params": {
       "workspace_id": "<workspace_id>",
       "title": "<role> (<agent_session_id>)",
       "cwd": "<worktree_path_or_cwd>",
       "command": "tmux attach-session -t ccx-<agent_session_id>",
       "envs": {
         "CCX_PROJECT_ID": "<id>",
         "CCX_AGENT_SESSION_ID": "<id>"
       }
     }
   }
   ```
   - **戻り値**: 新規作成された surface / tab ID (`cmux_tab_id` として SQLite に永続化)。

3. **タブ終了 (`surface.close`)**:
   ```json
   {
     "jsonrpc": "2.0",
     "id": "tab-close-1",
     "method": "surface.close",
     "params": {
       "surface_id": "<cmux_tab_id>"
     }
   }
   ```

4. **ユーザーへの通知・フラッシュ (`ui.notify`)**:
   エージェントがユーザーの介入を求めたり、重大なフェーズ変更を検出した際に、UI 上の Surface に notification ring を点灯させたり macOS 通知を発火する。
   ```json
   {
     "jsonrpc": "2.0",
     "id": "notify-1",
     "method": "ui.notify",
     "params": {
       "surface_id": "<cmux_tab_id>",
       "level": "warning",
       "message": "User Intervention Required: conflict detected in task.md"
     }
   }
   ```

#### 4.4.3 CmuxAdapter Trait の定義 (`src/agent_runtime/cmux_adapter.rs`)

```rust
pub trait CmuxAdapter: Send + Sync {
    /// cmux workspace を作成・または存在確認する
    fn ensure_workspace(&self, project_id: &str, display_slug: &str, canonical_repo: &str) -> Result<String, CcxError>;
    
    /// 新しい AgentSession 用のタブを cmux 内に作成し、tmux attach などの起動コマンドを指定する
    fn create_agent_tab(&self, spec: &AgentSessionSpec) -> Result<String, CcxError>;
    
    /// cmux の指定したタブを閉じる
    fn close_tab(&self, tab_id: &str) -> Result<(), CcxError>;

    /// 指定したタブ (Surface) に対して注意を惹く通知を発火する (リング点灯等)
    fn notify_user(&self, tab_id: &str, message: &str, level: &str) -> Result<(), CcxError>;
}

pub struct AgentSessionSpec {
    pub session_id: String,
    pub project_id: String,
    pub cmux_workspace_id: String,
    pub role: String,
    pub cwd_path: std::path::PathBuf,
    pub work_execution_id: Option<String>,
    pub worktree_path: Option<std::path::PathBuf>,
    pub envs: std::collections::HashMap<String, String>,
    pub startup_command: String, // 例: "tmux attach-session -t ccx-<session_id>"
}
```

#### 4.4.4 cmux/tmux 統合に関する決定事項

1. **PATH 消失問題対策 (GUI 側の環境変数引き継ぎ)**:
   - **CLI からの環境継承**: `ccx project open` や `ccx agent attach` などの CLI 起動時、現在のシェルの `PATH` を含む環境変数一式をソケット経由で `ccx-cmux` に引数として渡し、新規作成するターミナルタブに完全注入する。
   - **ログインシェルフォールバック**: GUI 自体が Finder/Dock から直接起動された場合に備え、フォーク版 `ccx-cmux` (Swift) 内部でバックグラウンドにてログインシェルを実行し (`zsh -l -c "printenv"`)、ユーザーのシェル環境変数を動的抽出してマージする処理を組み込む。

2. **リアルタイム状態同期方法 (State Sync Mechanics)**:
   - **ソケットイベントストリーム方式 (JSON-RPC Push)** を採用。
   - Controller (Rust) から `/tmp/cmux.sock` へのソケット接続を介し、コントローラー側で SQLite や JSONL に状態書き込み（イベント発生）があった場合は即座にソケットへ JSON-RPC 通知イベント `{"jsonrpc": "2.0", "method": "state.updated", "params": { ... }}` をプッシュ配信する。
   - `ccx-cmux` 側はソケットを常時リッスンし、`state.updated` 検知時に SQLite からデータを再読込して GUI を超高速で再描画する。

3. **Unix ソケットのセキュリティ (Socket Permissions)**:
   - `/tmp/cmux.sock` を作成する際、ファイルの所有者（実行ユーザー）のみが読み書きできるよう、ファイルのパーミッションを **`0600`** に明示的に制限し、ローカルの多重ユーザー環境における不正操作や乗っ取りを防ぐ。

4. **ターミナルのリサイズ & tmux 同期**:
   - `ccx-cmux` ターミナルタブがアタッチする際、`tmux attach-session -t ccx-<session_id> -d` (他のアタッチ端末を強制デタッチして自端末サイズに完全同期) を実行する。
   - tmux セッション作成時に `tmux set-window-option window-size largest` を明示設定し、非アクティブなアタッチ接続によって表示画面幅が極小化することを防止する。

5. **Upstream (cmux本家) とのフォーク同期手順 (git subtree)**:
   - モノレポ構成における `gui/` 配下の cmux ソースコードと本家との同期には **`git subtree`** を採用する。
   - **同期手順**: 本家の upstream リポジトリをリモート登録の上、`git subtree pull --prefix=gui upstream main --squash` を実行して、本家の修正や Ghostty バージョンアップを安全かつ容易に取り込む運用とする。

---

## 🖥️ 5. CLI (Control Interface) コマンド仕様

Orchestrator Agent と Worker Agent の境界を明確にし、長文のプロンプトや環境変数の安全な受け渡しを実現する CLI 仕様。

### 5.1 プロジェクト管理

- **登録**: `ccx project register --canonical-repo <path> --task-source-file <path>`
  - 新規 `project_id` (ULID) と `display_slug` を自動生成し、`project.json` と SQLite に記録。
  - **`project.json` 例**:
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
- **一覧**: `ccx project list [--json]`
- **開く**: `ccx project open <project_id> [--json]`
- **状況**: `ccx project status <project_id> [--json]`

### 5.2 エージェントセッション管理

- **Orchestrator 起動**: `ccx agent start-orchestrator --project-id <project_id>`
  - 特定の WorkExecution に依存しない、project-scoped の Orchestrator AgentSession を起動する。
- **Worker/他アタッチ**: 
  ```bash
  ccx agent attach \
    --work-execution-id <work_execution_id> \
    --role <worker|reviewer|diagnostic> \
    --mode <writer|reviewer|observer|diagnostic>
  ```
  - アタッチ時に、子プロセス（Harness）へ以下の環境変数を自動で注入する。
    - `CCX_PROJECT_ID`, `CCX_WORK_EXECUTION_ID`, `CCX_AGENT_SESSION_ID`
    - `CCX_WORKTREE_PATH`, `CCX_TASK_FILE`, `CCX_CLI`, `CCX_CANONICAL_REPO`
    - `CCX_ROLE`, `CCX_ATTACH_MODE`
    - *(※ `CCX_TASK_FILE` は、共有管理パスである `~/.ccx/projects/<project_id>/work-executions/<work_execution_id>/task.md` を指す)*
  - 投入すべき初期 prompt template をアタッチ完了時に自動投入する。
- **プロンプト送信 (メッセージファイル・stdin 対応化)**:
  ```bash
  ccx agent prompt --session-id <session_id> [--message <text>] [--message-file <path>] [--stdin]
  ```
  - 改行やコードブロックを含む長文プロンプトの送信時のシェルエスケープ事故を防ぐため、`--message-file` または `--stdin` を主要なデータ流路とする。
- **停止**: `ccx agent stop --session-id <session_id>`

### 5.3 WorkExecution (作業状態) 管理

- **作業状態作成**:
  ```bash
  ccx work create \
    --project-id <project_id> \
    --source-path <path> \
    --selector-type <type> \
    --selector-value <value> \
    --display-text <text>
  ```
  - 内部で以下のイベントを順次 JSONL に append する。
    1. `work_execution_created`
    2. `work_execution_task_file_created`
    3. `work_execution_state_changed` (`created` -> `task_file_created`)
  - *(※ `source_file_hash` は、Controller が実行時に自動で `source_path` から計算するため、CLI の引数としては指定不要である)*
  - **出力 JSON スキーマ**:
    ```json
    {
      "work_execution_id": "01HY...",
      "branch_name": "ccx/01HY...",
      "worktree_path": "~/.ccx/projects/01HY.../worktrees/01HY...",
      "task_file_path": "~/.ccx/projects/01HY.../work-executions/01HY.../task.md"
    }
    ```

- **作業状態クリーンアップ**:
  ```bash
  ccx work cleanup --work-execution-id <work_execution_id>
  ```
  - 当該 WorkExecution に属するアタッチセッション (tmux/cmux) および worktree を cleanup_policy に従って破棄。
  - **出力 JSON スキーマ**:
    ```json
    {
      "work_execution_id": "01HY...",
      "status": "cleaned_up",
      "removed_worktree": true,
      "closed_sessions": ["01HY_SESS1", "01HY_SESS2"]
    }
    ```

- **WriteLease 取得**:
  ```bash
  ccx lease acquire \
    --work-execution-id <work_execution_id> \
    --agent-session-id <agent_session_id> \
    [--force]
  ```
  - **出力 JSON スキーマ**:
    ```json
    {
      "write_lease_id": "01HY...",
      "status": "acquired"
    }
    ```

- **WriteLease 解放**:
  ```bash
  ccx lease release \
    --work-execution-id <work_execution_id> \
    --agent-session-id <agent_session_id>
  ```
  - **出力 JSON スキーマ**:
    ```json
    {
      "write_lease_id": "01HY...",
      "status": "released"
    }
    ```

- **マージ実行**:
  ```bash
  ccx merge execute --work-execution-id <work_execution_id> [--owner-agent-session-id <session_id>]
  ```
  - PR を merge 実行し、マージ完了後に安全な canonical repo pull を実行。
  - **`owner_agent_session_id` の解決ルール**:
    `--owner-agent-session-id` が明示的に指定された場合はその値をロック所有者として使用する。省略された場合は、Controller が対象プロジェクト内で稼働中の唯一の active な Orchestrator AgentSession を自動的に解決してロック所有者（owner）とする。
  - **出力 JSON スキーマ**:
    ```json
    {
      "work_execution_id": "01HY...",
      "status": "merged",
      "sync_status": "success",
      "sync_warning": null
    }
    ```

- **自律復旧診断**:
  ```bash
  ccx recovery digest --project-id <project_id> --json
  ```
  - 孤立セッションや stale な lease / lock を含むシステム整合性診断を返す。
  - **出力 JSON スキーマ**:
    ```json
    {
      "project_id": "01HY...",
      "timestamp": "2026-05-22T21:33:48Z",
      "diagnostics": {
        "active_sessions": 2,
        "stale_leases": [],
        "orphaned_tmux_sessions": [],
        "sqlite_dirty": false
      }
    }
    ```

### 5.4 データベース再構築

- **再構築**: `ccx db rebuild --project-id <project_id>`
  - SQLite が dirty（マーカーファイル `state/sqlite.dirty` 存在または `sqlite_dirty = 1`）の場合に再構築を実行し、完了後にマーカーファイルを削除。
- **整合性検証**: `ccx db verify --project-id <project_id>`

---

## 🛠️ 6. 詳細実装タスクリスト (実装順序に刷新した WBS)

開発・手動検証を迅速化するため、CLI Skeleton を早期に構築し、テストをモックベースの 3 段階に分けた実装順序に変更している。

### Phase 1: 開発基盤、IDモデル、共通ライブラリの実装

- [x] **1.1 Cargo.toml の正確な依存関係定義**
  - 以下の主要ライブラリを追加・構成する。
    - `ulid`, `serde` & `serde_json`, `serde_yaml`
    - `chrono`, `notify`, `rusqlite`
    - `thiserror`, `anyhow`, `clap`, `tokio` (full feature)
    - `fd-lock` (イベントログ排他ロック用)
    - `camino` (UTF-8 パスハンドリング用)
    - `directories` (ホームディレクトリ特定用)
    - `tracing` & `tracing-subscriber` (ログ出力用)
    - `tempfile` (テスト用テンポラリディレクトリ)
    - `assert_cmd` & `predicates` (CLI結合テスト用)
- [x] **1.2 共通エラー型 `CcxError` の定義 (`src/error.rs`)**
  - [x] `InvalidStateTransition { from, to }`
  - [x] `WriteLeaseConflict { active_session_id }`
  - [x] `MergeLockConflict`
  - [x] `TaskFileConflict` (同時編集競合発生時)
  - [x] その他 I/O、Git、DB 関連のエラーを網羅
- [x] **1.3 ULID 生成ユーティリティの実装 (`src/domain/event.rs`)**
  - [x] Monotonic ULID を生成する `generate_event_id()` を定義
- [x] **1.4 パス導出ユーティリティとプロジェクト登録コマンド (`src/cli/project.rs`)**
  - [x] `ccx project register` コマンドの実装
  - [x] `project.json` への永続化機能の実装

---

### Phase 2: Event 基盤 & JSONL 監査ログの実装

- [x] **2.1 Event 型および EventType の定義 (`src/domain/event.rs`)**
  - [x] 3.1節で定義したすべての `EventType` 列挙型の追加
  - 検証メモ: `EventData` の serde タグ付き union に加え、明示的な `EventType` enum と `EventData::event_type()` を実装済み。`cargo test event` で検証済み。
- [x] **2.2 JSONL append 機能の実装 (`src/persistence/jsonl.rs`)**
  - [x] `fd-lock` を用いた、プロジェクト固有の `events.lock` に対する排他書き込み処理の実装
  - [x] シリアライズされた JSON 文字列の追記、`flush()`, `sync_all()` 呼び出しの実装

---

### Phase 3: SQLite Projection (射影器) & Rebuild 機能

- [x] **3.1 SQLite DDL のマイグレーション実行 (`src/persistence/sqlite.rs`)**
  - [x] 2.4節で定義した CHECK 制約、UNIQUE 部分インデックス、`artifact_state` カラムを含む DDL の実装
- [x] **3.2 Projector (射影器) の実装 (`src/persistence/projector.rs`)**
  - [x] `Event` の受信時に SQLite の状態を変更するトランザクションロジックの実装
  - [x] `sqlite_dirty` フラグ管理の実装（書き込み失敗時に `state/sqlite.dirty` マーカーファイルを作成）
- [x] **3.3 Rebuild & Verify の実装 (`src/persistence/rebuild.rs`)**
  - [x] `ccx db rebuild` コマンドの実装。SQLite ファイルを初期化し、JSONL から状態を完全にロールフォワードする処理（完了後にマーカーファイルを削除）
  - [x] `ccx db verify` コマンドの実装。SQLite の整合性と JSONL の最新状態をクロスチェック

---

### Phase 4: Domain State (状態遷移 Validator)

- [x] **4.1 状態遷移 Validator の実装 (`src/domain/transition.rs`)**
  - [x] 3.2節の `validate_transition` の実装と単体テストの作成
  - [x] 不正な状態遷移を試みた場合に `CcxError::InvalidStateTransition` を返すか検証

---

### Phase 5: task.md (主成果物) & File Watcher 監視

- [x] **5.1 task.md Markdown テンプレートおよび YAML front matter 解析**
  - [x] `serde_yaml` を用いた front matter パース関数の実装
  - [x] パーサーに不完全なデータや不正な値が渡された場合の防御的バリデーション処理
- [x] **5.2 task_watcher 実装 (`src/watcher/task_watcher.rs`)**
  - [x] 各 WorkExecution の `task.md` (`work-executions/<id>/task.md`) の更新（変更検出）を監視する。
  - [x] コンテンツのコンテンツハッシュ (SHA-256) を計算し、`last_seen_hash` との重複検知 (Deduplication) を行う仕様の実装。同一ハッシュならイベント生成をスキップ。
  - [x] front matter を best effort で解析する。
  - [x] `work_execution_task_file_changed` イベントを生成し、JSONL（監査ログ正本）へ append する。
  - [x] status に変化がない場合は、通知の優先度 (priority) を下げる、または不要な通知をスキップするフィルタリング処理の実装。
  - [x] Projector がイベントを受けて SQLite に反映（射影）する。
  - [x] 最終的に Orchestrator Agent に変更を通知（チャネル経由など）する。
- [x] **5.3 source_watcher 実装 (`src/watcher/source_watcher.rs`)**
  - [x] 業務正本である task source file の更新を検知し、`task_source_file_changed` イベントを記録して Orchestrator に伝える仕組みの実装

---

### Phase 6: Git Local 操作モジュール

- [x] **6.1 canonical repo 制御 (`src/git/repo.rs`)**
  - [x] canonical repo 用の dirty check (`git status --porcelain` 実行と解析)
- [x] **6.2 worktree & branch 管理の実装 (`src/git/worktree.rs`)**
  - [x] リポジトリ外への `git worktree add` およびブランチ生成（作成時に `worktree_created`, `branch_created` イベントを JSONL に append。また、worktree から管理下の `task.md` への symlink 設置処理も含む）
  - [x] cleanup 時に worktree を削除し、prune を行う処理の実装

---

### Phase 7: CLI Skeleton の構築

- [x] **7.1 Clap サブコマンド骨格の実装**
  - [x] `main.rs` および `src/cli/` 以下にすべてのサブコマンドのパースインターフェースを実装
  - [x] `work create` / `work cleanup` / `lease acquire` / `lease release` / `merge execute` / `recovery digest` を含むすべてのコマンドがダミーデータを `--json` で出力できるスケルトンを作成

---

### Phase 8: Agent Runtime (cmux / tmux アダプター & 協調ライフサイクル)

- [x] **8.1 Trait `CmuxAdapter` の定義と Unix Socket / CLI 実装**
  - [x] `src/agent_runtime/cmux_adapter.rs` の作成および 4.4.3 節の Trait 定義
  - [x] Unix ドメインソケット `/tmp/cmux.sock` への JSON-RPC クライアント実装 (ワークスペース作成、タブ追加、タブ閉鎖、通知)
  - [x] `cmux` CLI 経由のコマンド発行によるフォールバック動作の実装
  - [x] macOS 以外の環境や `cmux` アプリ未起動時に、エラーを出さずに headless 実行へ移行するフォールバックロジックの実装
- [x] **8.2 バックグラウンド tmux アダプターと Harness 実装**
  - [x] `tmux_adapter.rs` によるバックグラウンド tmux session の起動、アタッチ、停止の実装
  - [x] `harness.rs` による子プロセスの PID、cwd 監視、および PTY I/O 等に基づく heartbeat 更新
  - [x] `agent attach` 実行時に、所定の 9 つの環境変数を子プロセス環境に注入する処理の実装
- [x] **8.3 cmux / tmux の協調ライフサイクル制御**
  - [x] 新規セッション開始時、まず `tmux` バックグラウンドセッションを作成し、続いて `cmux` タブを開いて `tmux attach` を走らせる二段階起動シーケンスの実装
  - [x] `ccx agent stop` 時、または cmux タブが閉じられたことを検知した際に、対応する tmux セッションも安全に終了させる同期ロジック
- [x] **8.4 プロンプト送信・ファイル注入の実装 (`src/agent_runtime/prompt.rs`)**
  - [x] `--message-file` および `--stdin` を読み込み、対象 tmux 端末へキー入力送信する処理の実装
- [x] **8.5 Cmux UI 連携とユーザー通知機能 (`ccx agent notify`)**
  - [x] AI エージェントの作業状況、または `user_intervention` 要求を cmux の UI に通知する `ui.notify` API 連携の実装
  - [x] CLI サブコマンド `ccx agent notify --session-id <id> --message <msg> [--level <info|warning>]` の追加
- [x] **8.6 Agent lifecycle hook / exit hook の抽象化と実装**
  - [x] Agent 実装固有の stop / end event を `AgentLifecycleStop` イベントに写像する。
  - [x] lifecycle hook 発火時に `task.md` の存在および非空（0バイトでないこと）を確認・検証する。
  - [x] 検証成功時に `AgentLifecycleStop` イベントを JSONL 監査ログに記録し、かつ SQLite の `artifact_state` を `ready` に（失敗時は `invalid` に）投影更新する。
  - [x] イベント発生を Orchestrator Agent に通知する。

---

### Phase 9: PR / Hook / マージ自動化フロー

- [x] **9.1 gh-review-hook 実行の実装 (`src/git/review_hook.rs`)**
  - [x] worktree を cwd としたフック起動と exit code (`0`, `2`, その他) の分類評価
- [x] **9.2 マージ実行アルゴリズムの実装 (`src/git/github.rs`)**
  - [x] 14.4節の「マージ実行フロー」に基づくマージトランザクションの実装
  - [x] 4.3節の「安全な Canonical Repo 同期処理」の実装。sync_warning の制御ロジック

---

### Phase 10: 自律復旧 & Circuit Breaker

- [x] **10.1 `recovery digest` 機能の実装**
  - [x] 整合性チェック（SQLite情報と tmux 端末/PID/Git状態のクロスチェック）
  - [x] 孤立セッション自動検出、stale な write lease や merge lock の検出処理
- [x] **10.2 Circuit Breaker 機能の実装**
  - [x] リトライ回数閾値・実行時間の自動評価、閾値到達時の `hold` 自動遷移

---

### Phase 11: クリーンアップポリシー制御

- [x] **11.1 クリーンアップポリシーの実装 (`src/cli/work.rs`)**
  - [x] `cleanup_policy` (immediate | keep_last_n | keep_for_duration) の設定に応じた、アタッチセッション (tmux/cmux) および worktree の破棄ロジックの実装

---

### Phase 12: 3段階 E2E テストと検証

- [x] **12.1 Level 1: Pure / Local Tests**
  - [x] `validate_transition` 状態遷移の単体テスト
  - [x] JSONL ファイル書き込みロック、SQLite マイグレーション、front matter パーサーの単体テスト
- [x] **12.2 Level 2: Local Git Tests**
  - [x] ローカルの git ベアリポジトリに対する worktree の作成・削除、ブランチ作成、dirty check、reset のテスト
- [x] **12.3 Level 3: Fake External Command Tests**
  - [x] テスト用フィクスチャとして `tests/fixtures/bin/gh` および `tests/fixtures/bin/gh-review-hook` のフェイクスクリプトを作成
  - [x] `gh pr view` や `gh pr merge` のダミー応答を介して、PR状態・SHAの不一致・フック exit code に応じるコントローラーの一連の挙動（E2E）を全自動でテストするスクリプトを構築
- [x] **12.4 本番 GitHub 結合テストの実行 (マニュアル実行 / Nightly 用)**
  - [x] 実際の GitHub に対して PR の作成・フック検証・自動マージができるかを手動検証

---

### Phase 13: cmux フォークによる CCX 専用 GUI の実装

- [x] **13.1 cmux リポジトリのフォークおよびビルド環境の構築**
  - [x] `cmux` (Swift/AppKit/libghostty ベース of macOS アプリ) のソースコードをフォークし、Xcode を用いたローカル開発・ビルド環境をセットアップする
- [x] **13.2 コントローラーおよび永続化データとのデータ連携**
  - [x] フォークした Swift アプリケーション内に、CCX Controller が生成する SQLite (`state.sqlite`) および JSONL ログファイルを読み込み・監視する機能を実装する
- [x] **13.3 CCX 専用 ダッシュボード / サイドバー GUI の実装 (SwiftUI/AppKit)**
  - [x] `ccx_requirements_parallelized_v3.md` の「7.2 表示領域」に準拠した統合管理画面を実装する
    - [x] **Overview**: プロジェクトの状態、アクティブエージェント数、Hold/Merge_ready セッション数などのメタ情報をリアルタイム表示するダッシュボード
    - [x] **WorkExecutions / AgentSessions**: 現在稼働中の Worktree、ブランチ、各セッションの役割、アクティブな write lease 状態の可視化
    - [x] **Reviews / Merges**: 自動レビュー結果 (gh-review-hook のステータス・エラーログ) およびマージキューの表示
    - [x] **Artifacts / Events**: task.md、差分要約、および JSONL から射影されたタイムライン表示
- [x] **13.4 カスタムタブ / マルチ Surface の統合レイアウト**
  - [x] 各 AgentSession に対応する Ghostty ターミナルサーフェスと、作業タスク進捗状況などを一覧できるサイドパネルを、美しくレスポンシブなスプリット表示として統合
- [x] **13.5 コントローラー CLI からのシームレスな起動連携**
  - [x] `ccx project open` コマンドが実行された際に、フォークした CCX-cmux アプリを `open -a ccx-cmux --args --project-id <id>` のように引数付きで呼び出し、自動で当該プロジェクトのワークスペースを開く起動連携を実装する
- [x] **13.6 Xcode / cmux 本体への組み込み**
  - [x] `gui/Sources/CCX/*.swift` を `gui/cmux.xcodeproj/project.pbxproj` の main app target に組み込み (PBXFileReference / PBXBuildFile / PBXSourcesBuildPhase) して実ビルドを通す (`plutil -lint` 通過)
  - [x] cmux 既存の `PanelType` enum に `.ccxDashboard` ケースを追加し、 `PanelContentView` / `Workspace` / `CmuxLifecycleEventPublishing` 等の網羅 switch を更新 (`CCXDashboardPanel` + `CCXDashboardPanelView` で配線)
- [x] **13.7 ccx-cmux ブランド化と起動フック**
  - [x] `PRODUCT_NAME` / `PRODUCT_BUNDLE_IDENTIFIER` を `ccx-cmux` (Release) / `ccx-cmux DEV` (Debug) / `com.cmuxterm.ccx-cmux[.debug]` へリネーム
  - [x] `gui/scripts/reload.sh`、`gui/scripts/cmux-debug-cli.sh`、`gui/scripts/run-tests-v{1,2}.sh`、`gui/scripts/smoke-test-ci.sh` などのスクリプトを `ccx-cmux DEV` / `com.cmuxterm.ccx-cmux.debug` に追随更新
  - [x] `AppDelegate.applicationDidFinishLaunching` から `CCXAppDelegateBridge.presentDashboardIfRequested(on:)` で `CCXLaunchArguments.parse()` を呼び出し、`Workspace.newCCXDashboardSurface(inPane:projectId:)` 経由でダッシュボードを開く起動フックを実装
  - [x] `src/cli/project.rs::open` の Launch Services 検索を `mdfind` から `open -a` フォールバック方式に切り替え、`~/Applications` などインデックス外のインストール先でも解決可能に
  - [x] **実機検証**: `ccx-cmux DEV.app` を `xcodebuild` でビルド → `~/Applications` に配置 → `ccx project open <id>` でダッシュボードが開くことを `cmux-debug-*.log` の `ccx.dashboardOpen source=ccxLaunchArgs` で確認

---

### Phase 14: GUI からのプロジェクト管理 (Project Management UX)

現状の `ccx-cmux` GUI は単一プロジェクトの read-only ダッシュボードであり、プロジェクト登録・一覧・切替・削除はすべて `ccx project register|list|open|...` の CLI 経由でしか実行できない。Worker / Orchestrator 体験を上げるため、GUI 自身でプロジェクト一覧・登録・切替が完結できるようにする。

> [!NOTE]
> Event sourcing 不変条件を保つため、GUI からの全ミューテーションは GUI 内で events.jsonl / projects.json を直接書かず、`CCX_CLI` (CCX_BUNDLED_CLI_PATH or PATH から解決) を `Process` API でサブプロセス起動して実行する。GUI 自身は引き続き SQLite / JSONL の read-only コンシューマ。

- [x] **14.1 CLI ランチャー抽象 (`gui/Sources/CCX/CCXControllerCLI.swift`)**
  - [x] `ccx` バイナリのパスを解決する (起動時の `CCX_CLI` 環境変数 → `$CCX_HOME/bin/ccx` → `PATH` 探索の優先順)。バイナリが見つからない場合はエラー表示用の Result を返す
  - [x] `register(canonicalRepo:taskSourceFile:) async throws -> CCXProjectSummary` のように `--json` 出力をパースする型安全ラッパーを実装
  - [x] サブプロセス stderr を保持し、CLI 失敗時に GUI 側のエラートーストで原因を提示する
- [x] **14.2 全プロジェクト一覧モデル (`CCXProjectsStore`)**
  - [x] `~/.ccx/projects.json` を `FSEventStream` で監視し、プロジェクト追加・削除を即時反映する `@Observable` モデルを新設
  - [x] 既存 `CCXProjectStore` (単一プロジェクト) と同等に、I/O は detached task でバックグラウンド実行し、状態更新のみ MainActor
  - [x] 各 entry を `CCXProjectSummary` として公開し、起動済みの単一プロジェクトダッシュボードと相互運用
- [x] **14.3 Project Picker サイドバー / Welcome 画面**
  - [x] `--project-id` 無しで起動した場合に表示する Project Picker を実装 (プロジェクト一覧、選択で新規 `CCXDashboardPanel` を adopt、"+ Add Project" 行で 14.4 の登録シートを表示)
  - [x] サイドバー上部に現在のプロジェクト切替メニュー (NSMenu または SwiftUI Menu) を追加し、選択時は新しいタブで該当プロジェクトのダッシュボードを開く
- [x] **14.4 新規プロジェクト登録 Sheet**
  - [x] `NSOpenPanel` でリポジトリパスと task source file (`*.md`) を選択させるフォーム UI を実装
  - [x] バリデーション (`.git` ディレクトリ存在 / task source file 存在) を GUI 側で先行チェックし、`ccx project register` をサブプロセス実行して結果を取り込む
  - [x] 登録成功時、即座に新規プロジェクトのダッシュボードに切り替える
- [x] **14.5 プロジェクト切替 (`Workspace.switchToCCXDashboard(projectId:)`)**
  - [x] 既存ワークスペース内の `CCXDashboardPanel` を破棄して別 projectId のパネルに置換するヘルパーを `Workspace` に追加
  - [x] サイドバー切替時にこのヘルパーを呼び、`publishCmuxSurfaceClosed` → 新規 `ccxDashboard` surface created を順序保証して発火
- [x] **14.6 プロジェクト削除 / Unregister**
  - [x] サイドバー行の右クリックメニューに "Unregister project" を追加し、確認ダイアログ後に `ccx project unregister <id>` をサブプロセス起動 (まだ未実装なので CLI 側にも追加が必要)
  - [x] Controller CLI 側に `ccx project unregister --project-id <id> [--purge]` を実装し、`projects.json` から該当行削除、`~/.ccx/projects/<id>/` を `--purge` 指定時のみ削除、events.jsonl に `project_unregistered` を append
- [x] **14.7 起動引数ポリシーの整理 (`CCXLaunchArguments`)**
  - [x] `--project-id` 省略時は Project Picker を表示、`--project-id` 指定時は従来通り直接ダッシュボードを開くよう既存の `CCXAppDelegateBridge.presentDashboardIfRequested(on:)` を更新
  - [x] 環境変数 `CCX_DEFAULT_PROJECT_ID` 経由でデフォルトプロジェクトを設定できるオプションも追加
- [x] **14.8 実機検証**
  - [x] 新規ビルドの `ccx-cmux DEV.app` から:
    1. Project Picker 表示 (引数なし起動)
    2. "+ Add Project" でリポジトリ選択 → 登録 → ダッシュボード遷移
    3. サイドバー切替で別プロジェクトに移動
    4. Unregister 動作 (`projects.json` から消えることを確認)
  - [x] CLI と GUI を交互に操作しても `projects.json` / `state.sqlite` が破綻しないことを `ccx db verify` で確認

---

### Phase 15: GUI からの Task Source 管理 / LLM Task Intake

現状の `ccx-cmux` GUI には、プロジェクト登録時に `task source file` を指定する導線と、Dashboard 上でその path を表示する導線しかない。Task の指定・閲覧・編集・追記は GUI から実行できず、`work create` も task source file から WorkExecution を作る skeleton に留まっている。Project Management UX を実運用に乗せるため、GUI から task source file を扱い、自然言語の作業依頼を Orchestrator Agent 経由で詳細化して task source file に反映できるようにする。

> [!NOTE]
> `task source file` は業務上の作業管理の正本であり、Controller は業務的意味を解釈しない。LLM による詳細化・追記は GUI が直接ファイルを書き換えるのではなく、原則として Orchestrator Agent に prompt 投入し、Orchestrator Agent がコード確認と task source file 更新を行う。GUI 側の直接保存は、明示的な手動編集として hash / mtime 競合検知を通す。

- [x] **15.1 現状導線の明示と Tasks タブ追加**
  - [x] `CCXDashboardView` に **Tasks** タブを追加し、登録済み `taskSourceFile` の path、最終読込時刻、読込エラーを表示する
  - [x] `task source file` が未設定・存在しない・Markdown でない場合の空状態 / エラー状態を表示する
  - [x] `Open in Editor` / `Reveal in Finder` / `Copy Path` の導線を追加する
- [x] **15.2 Task Source 読み書き CLI**
  - [x] `ccx task-source read --project-id <id> --json` を追加し、path、content、hash、mtime を返す
  - [x] `ccx task-source write --project-id <id> --expected-hash <hash> --stdin --json` を追加し、競合時は non-zero と明示エラーを返す
  - [x] `ccx task-source append --project-id <id> --expected-hash <hash> --stdin --json` を追加し、追記位置と更新後 hash を返す
  - [x] task source file が canonical repo 内にある場合、dirty state が発生することを JSON warning として返す
- [x] **15.3 GUI 手動編集ビュー**
  - [x] `CCXControllerCLI` に task source read/write/append の型安全 wrapper を追加する
  - [x] `CCXTaskSourceStore` を追加し、読み込み、保存、競合検知、再読込を MainActor 安全に扱う
  - [x] Tasks タブに Markdown テキスト編集領域、保存、再読込、変更破棄の導線を実装する
  - [x] 保存時に `expected-hash` 不一致なら上書きせず、差分確認または再読込を促す
- [x] **15.4 自然言語 Task 追加 Composer**
  - [x] Tasks タブに自然言語入力欄を追加し、「LLM で詳細化して追加」操作を提供する
  - [x] 入力には task source file、canonical repo、現在の WorkExecution 状態、希望する追加形式を含めた Orchestrator 向け prompt template を適用する
  - [x] Orchestrator Agent session が存在しない場合は開始し、存在する場合は `ccx agent prompt` で依頼を投入する
  - [x] prompt には「コードを確認し、必要なら task を分割・詳細化し、task source file に反映する。GUI からの原文も残す」ことを明記する
- [x] **15.5 Orchestrator 反映フロー**
  - [x] Orchestrator Agent が task source file 更新後、Controller の source watcher が `task_source_file_changed` を記録し、GUI が再読込できることを確認する
  - [x] LLM 詳細化の結果として追記された task の見出し / チェックボックス / anchor を GUI で確認できるようにする
  - [x] 失敗時は Orchestrator Agent からのエラー、未反映理由、追加で必要な情報を GUI に表示する
- [x] **15.6 WorkExecution 作成連携**
  - [x] GUI 上で task source file の見出し / チェックボックス候補方式（raw selection range を採用しない）で選び、`ccx work create` に渡せるようにする
  - [x] `work create` の skeleton を実装し、WorkExecution、task.md、branch、worktree、イベント、SQLite projection が実際に作成されるようにする
  - [x] 作成した WorkExecution に Orchestrator / Worker Agent を attach / prompt できる導線を追加する
- [ ] **15.7 検証**
  - [x] CLI 単体テスト: read/write/append、hash 競合、missing file、dirty warning
  - [x] GUI unit test: Tasks タブ状態、保存成功、競合表示、Composer prompt 生成
  - [x] fake `ccx` / fake Orchestrator で自然言語 task 追加の E2E を通す
  - [ ] 実機検証: GUI から「自然言語で task 指定 → Orchestrator がコード確認 → 詳細化 task を task source file に反映 → GUI が再読込」を確認する

---

### Phase 16: オーケストレーション 完全フローの欠損補完（GUI起点）

ストーリーで要求された一連操作が断絶なく成立するため、現在未整備または部分実装のままの機能を埋める。

- [ ] **16.1 GUI 起動後のワークフロー一貫化（起動 → 開封）**
  - [ ] GUI 起動時に、直近プロジェクト/デフォルトプロジェクトを自動解決してダッシュボードを開く UX を確認し、見つからない場合は明示的な「Open Project」導線を最短1タップで提示する
  - [ ] `ccx project open` と GUI 内 `Workspace.switchToCCXDashboard(projectId:)` の経路差分を吸収し、同一実行経路で `taskSourceFile` と `state.sqlite` が確実に読み込まれることを担保
  - [ ] ダッシュボード起動時に Orchestrator の attach 状態を取得・表示し、未起動時はアクションから起動可能にする

- [ ] **16.2 タスク追加を確実に Orchestrator で反映するためのガード**
  - [x] 自然言語入力の Composer が空・巨大・不正 YAML/Markdown 文字列を弾くバリデーションを GUI 側で実装（必須項目/サイズ/文字種の事前整合）
  - [x] `submitNaturalLanguageTask` から投入される prompt に「元タスク保持」「差分不可欠」「競合時の再実行手順」を常時含めるプロンプトテンプレートを固定化
  - [x] Orchestrator が起動中でない場合の例外ケースを統一ハンドリングし、起動失敗時に再試行可能な UI フローを追加（自動起動 + 失敗理由の明示）
  - [x] Orchestrator への依頼後、`task_source_file_changed` を監視して GUI 自動リフレッシュするイベント配線を追加し、反映漏れを検知できる timeout 判定を導入

- [ ] **16.3 実行中ワーカーの制御拡張（指示・停止・再開）**
  - [x] `ccx agent stop --session-id` を GUI アクションとして再公開するための CLI ラッパーを追加（`CCXControllerCLI.stopAgent` + `CCXAgentStopResult`）
  - [x] WorkExecutions タブに停止ボタンを追加し、`ccx agent stop` 実行結果を行内で成功/失敗表示する
  - [x] `ccx agent attach` または新規 `work create` 経路による「ワーカー起動（再起動含む）」ボタンを実装し、既存 orchestrator/worker セッションの紐付けを明示
  - [ ] `agent prompt` を用いた追加指示投入をタスクごとに繰返し行える UI を追加（ワーカー名、セッションID、直近指示履歴、送信結果表示）
  - [ ] GUI の WorkExecution 行に「停止」「再起動」「追加指示」の3種を揃え、停止後の状態遷移 (`running` → `stopping` → `canceled/returned`) 可視化

- [ ] **16.4 ワーカー完了時の branch / tasks.md 自動更新（同期性）**
  - [ ] ワーカー完了通知と `work_execution` state 変化（`merged`/`failed`）を Orchestrator が受けるループを実装し、完了後の follow-up テンプレートを明示
  - [ ] `merged` 確定時に canonical ブランチ更新後の差分から tasks.md 対象セクションを更新する Orchestrator 用タスクを追加（例: Result/Blockers/Remaining Work）
  - [ ] `followup_required` を明示している場合はデフォルト tasks.md と WorkExecution の両方へ反映されることを検証し、更新失敗時は通知で再実行可能状態を保持
  - [ ] worker 完了イベント→tasks.md 更新→GUI リフレッシュの1トランザクション化を目指し、重複実行防止用の idempotency key（work_execution_id）を持たせる

- [ ] **16.5 監査ログ・再現性・検証**
  - [ ] 15.7 の実機検証に加え、上記16.x の GUI E2E を追加（起動・ワーカー起動・追加指示・停止・完了・tasks.md 反映）
  - [ ] ストーリー通りのシナリオテストを 1件追加：`起動 → open → 自然言語追加 → orchestrator 反映 → ワーカー停止/再開 → 完了反映`
  - [ ] 手順失敗時のログと復旧手順を `z/tasks.md` から追跡可能なように、失敗モード別の観測項目（CLI exit code, event type, session id, file hash）を追記

## 📝 7. 個別 task.md 設計テンプレート (Worker Agent 向け)

各 WorkExecution の作成時に `task.md` として生成される Markdown の構造。

```markdown
---
project_id: <ULID>
work_execution_id: <ULID>
status: assigned
source_path: <task_source_file_path>
source_ref: <work_item_ref>
branch: ccx/<work_execution_id>/<slug>
pr_number:
pr_url:
head_commit:
gh_review_hook_exit_code:
current_writer_session_id:
updated_by: orchestrator
updated_at: <ISO-8601>
---

# Work Item

## Original Task
<!-- Orchestrator が切り出した元のタスク記述 -->

## Instructions
<!-- Worker Agent への具体的な実装指示や境界条件 -->

## Progress
<!-- Worker が進捗状況を追記 -->

## Pull Request
<!-- 作成された PR 情報を追記 -->

## Review / Gate
<!-- サブエージェントレビューや gh-review-hook の結果を追記 -->

## Result
<!-- 最終成果の要約 -->

## Remaining Work
<!-- 次回へ引き継ぐ残作業や follow-up 事項 -->

## Blockers
<!-- 進行を阻害している問題や、差戻しの理由 -->
```
