# Seahorse 战略级落地方案（最终融合版）

## 新一代认知记忆引擎 —— 让 AI Agent 拥有类人记忆

---

## 〇、Executive Summary

**一句话定位：** Seahorse 是全球首个将神经科学启发的动态记忆机制原生融入 RAG 架构的生产级 AI Agent 认知记忆引擎。

**核心价值主张：**

- **从被动检索到主动涌现**：突破传统向量数据库的静态 Top-K 范式，实现联想式记忆召回
- **从扁平空间到认知拓扑**：通过脉冲神经网络和语义引力场构建多层次记忆组织
- **从单次查询到持续演化**：记忆系统随使用自适应优化，越用越智能

**战略差异化壁垒：**

| 维度 | 传统方案 | Seahorse |
|------|----------|----------|
| 检索范式 | 余弦相似度 Top-K | LIF 脉冲扩散 + Tide 能量分解 + 语义引力场 |
| 记忆结构 | 扁平向量空间 | Tag 共现拓扑 + 多跳联想网络 + 世界观分区 |
| 意图理解 | 单向量匹配 | Gram-Schmidt 多层能量剥离 + 投影熵分析 |
| 记忆演化 | 静态索引 | 突触可塑性 + 梦境整合 + 自适应压缩 |
| 弱信号捕获 | 无 | 残差空间挖掘被掩盖的隐含记忆 |
| 部署形态 | 云服务 / Python 库 | Rust 核心 + 多语言 SDK + WASM + 嵌入式 |

**18 个月战略目标：**

1. **协议层**：成为 AI Agent 记忆交互的事实标准（MCP 集成）
2. **引擎层**：达到百万级向量、毫秒级检索的生产性能
3. **生态层**：集成主流 Agent 框架（LangChain/LlamaIndex/CrewAI），建立记忆市场

---

## 一、战略架构：仿脑三层认知模型

### 1.1 全局架构视图

```
┌─────────────────────────────────────────────────────────────────┐
│                    Layer 3: 生态协议层                            │
│  ┌──────────────┬──────────────┬──────────────┬──────────────┐  │
│  │ MCP Protocol │ LangChain    │ Agent        │ Memory       │  │
│  │ Server       │ Integration  │ Frameworks   │ Marketplace  │  │
│  └──────────────┴──────────────┴──────────────┴──────────────┘  │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────┴────────────────────────────────────┐
│                    Layer 2: 接口抽象层                            │
│  ┌──────────────┬──────────────┬──────────────┬──────────────┐  │
│  │ Python SDK   │ Node.js      │ REST/gRPC    │ WASM         │  │
│  │ (PyO3)       │ (napi-rs)    │ (axum)       │ Browser      │  │
│  └──────────────┴──────────────┴──────────────┴──────────────┘  │
└────────────────────────────┬────────────────────────────────────┘
                             │ FFI Bridge (零开销抽象)
┌────────────────────────────┴────────────────────────────────────┐
│                Layer 1: 认知内核层 (Pure Rust)                    │
│                                                                   │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              Cerebral Cortex (大脑皮层)                     │  │
│  │        向量空间管理 + 增强型 HNSW 索引                       │  │
│  │  • mmap 持久化  • 动态量化  • 引力场辅助路由                │  │
│  └───────────────┬──────────────┬────────────────────────────┘  │
│                  │              │                                │
│  ┌───────────────▼──────────────▼────────────────────────────┐  │
│  │              Synaptic Network (突触网络)                    │  │
│  │        LIF 脉冲扩散引擎 + Tag 共现拓扑                        │  │
│  │  • 多跳联想  • 涌现模式检测  • 传播控制  • STDP 可塑性     │  │
│  └───────────────┬──────────────────────────────────────────┘  │
│                  │                                                │
│  ┌───────────────▼──────────────────────────────────────────┐  │
│  │              Thalamic Gate (丘脑门控)                      │  │
│  │        Tide 算法引擎 + 意图理解                              │  │
│  │  • Gram-Schmidt 能量分解  • 投影熵  • 世界观分类  • 去重   │  │
│  └───────────────┬──────────────────────────────────────────┘  │
│                  │                                                │
│  ┌───────────────▼──────────────────────────────────────────┐  │
│  │              Hippocampus (海马体)                           │  │
│  │        统一存储引擎 (SQLite + mmap + WAL)                   │  │
│  │  • Schema 管理  • 事务支持  • 增量备份  • 迁移工具         │  │
│  └───────────────┬──────────────────────────────────────────┘  │
│                  │                                                │
│  ┌───────────────▼──────────────────────────────────────────┐  │
│  │              Cerebellum (小脑)                              │  │
│  │        后台任务调度引擎                                      │  │
│  │  • 梦境整合  • 记忆压缩  • 拓扑维护  • 健康分析            │  │
│  └──────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────┘
```

### 1.2 仿脑命名的架构约束

**设计哲学：** 每个“脑区”组件严格遵守单一职责原则，边界清晰，不可越权。

| 脑区组件 | 生物类比 | 职责边界 | 禁止行为 |
|---------|---------|---------|---------|
| **Cortex** | 大脑皮层 | 向量空间表示、HNSW 图管理、距离计算 | ❌ 不处理 Tag 拓扑关系 |
| **Synapse** | 突触网络 | LIF 脉冲传导、共现矩阵维护、多跳扩散 | ❌ 不做向量检索 |
| **Thalamus** | 丘脑 | 信号门控、能量分解、意图路由 | ❌ 不存储状态 |
| **Hippocampus** | 海马体 | 持久化存储、事务管理、Schema 演化 | ❌ 不做计算 |
| **Cerebellum** | 小脑 | 异步任务调度、后台优化 | ❌ 不阻塞主路径 |

---

## 二、技术栈选型矩阵

```
┌──────────────┬────────────────────┬──────────────┬─────────────────────────┐
│   技术维度    │      首选方案       │   备选方案    │       决策理由            │
├──────────────┼────────────────────┼──────────────┼─────────────────────────┤
│ 核心语言      │ Rust 2021 Edition  │ -            │ 零成本抽象+内存安全+性能   │
│ 向量索引      │ 自实现 HNSW         │ usearch      │ 深度定制 LIF 层+引力场路由   │
│ 线性代数      │ nalgebra 0.33      │ faer         │ 泛型维度+no_std+成熟生态  │
│ 稀疏矩阵      │ sprs 0.11          │ sparse-mats  │ CSR 格式成熟+文档完善      │
│ 持久化        │ SQLite (rusqlite)  │ redb/sled    │ 生态成熟+跨平台+ACID      │
│ 序列化        │ rkyv (零拷贝)      │ bincode      │ mmap 场景关键+极致性能     │
│ 异步运行时    │ tokio 1.x          │ async-std    │ 生态统一+tower 集成        │
│ Python 绑定   │ PyO3 0.22+         │ cffi         │ 类型安全+异步支持+性能    │
│ Node.js 绑定  │ napi-rs 3.x        │ ffi-napi     │ N-API 稳定+TS 支持        │
│ WASM 编译     │ wasm-bindgen       │ wasm-pack    │ 浏览器端记忆+隐私计算     │
│ HTTP 框架     │ axum 0.7           │ actix-web    │ tokio 原生+类型安全路由    │
│ gRPC          │ tonic              │ grpc-rs      │ async/await 原生+生态好    │
│ 可观测性      │ tracing + metrics  │ opentelemetry│ 结构化日志+Prometheus 兼容 │
│ 测试框架      │ cargo-nextest      │ cargo-test   │ 并行测试+更好输出         │
│ 性能基准      │ criterion + divan  │ -            │ 统计严谨+微基准支持       │
│ 向量化        │ wide (SIMD)        │ std::simd    │ 稳定+跨平台               │
│ 嵌入式 C-API  │ cbindgen           │ bindgen      │ 自动生成头文件+无运行时   │
└──────────────┴────────────────────┴──────────────┴─────────────────────────┘
```

---

## 三、认知内核详细设计

### 3.1 Cortex：增强型 HNSW

**核心职责：**
- 负责向量检索主路径
- 支持 LIF/引力场上下文修正距离
- 支持 mmap + rkyv 零拷贝持久化

**关键设计要点：**
1. **动态边权重**：边权重由距离、共现强度、访问时间衰减、LIF 电位影响共同决定。
2. **检索路径可注入上下文**：`lif_states` 与 `gravity_field` 均可影响路径选择与排序。
3. **持久化不可阻塞**：写入时采用异步快照与增量落盘，避免阻塞主检索。

**实现约束：**
- 数据结构必须可序列化为 rkyv
- 向量距离计算必须统一入口，避免多处数值误差

**关键数据结构（示意）：**
```rust
struct HnswGraph {
    layers: Vec<Layer>,
    vectors: HashMap<NodeId, VectorEntry>,
    node_levels: HashMap<NodeId, usize>,
    entry_point: Option<NodeId>,
    config: HnswConfig,
}

struct Edge {
    target: NodeId,
    base_distance: f32,
    cooccur_strength: f32,
    lif_affinity: f32,
    recency_factor: f32,
    last_traversed: i64,
}

impl Edge {
    fn effective_distance(&self, lif: Option<f32>) -> f32 {
        let mut dist = self.base_distance;
        dist *= 1.0 / (1.0 + 0.5 * self.cooccur_strength);
        if let Some(p) = lif {
            dist *= 1.0 - 0.3 * p.max(0.0).min(1.0);
        }
        dist * self.recency_factor
    }
}
```

**检索路径修正公式（统一入口）：**
```
距离' = distance × (1 - α×LIF_potential) × gravity_weight × recency_factor
```

**持久化策略细节：**
- HNSW 图结构 → rkyv 序列化 → mmap 文件
- 写入流程：内存更新 → 异步快照 → mmap 原子替换
- 读流程：mmap 映射 → rkyv 校验 → 直接访问归档数据

---

### 3.2 Synapse：LIF 脉冲扩散引擎

**核心职责：**
- 基于 Tag 共现拓扑进行联想扩散
- 提供多跳检索与涌现模式识别
- 控制扩散规模，避免雪崩式激活

**完整流程：**
1. **种子识别**：query 向量投影到 Tag 质心 → Top-K 作为种子
2. **脉冲扩散**：按 hop 扩散，电位衰减
3. **收敛检测**：激活数量变化率低于阈值即停止
4. **全局抑制**：激活过多则整体衰减
5. **涌现模式检测**：无直接边但被激活的 Tag 组合被标记为 emergent

**实现约束：**
- 必须记录 SpikeTrace（用于可视化与可调优）
- 每次传播后必须更新可塑性统计

**关键数据结构（示意）：**
```rust
pub struct LIFNeuron {
    pub tag_id: TagId,
    pub v: f32,
    pub v_rest: f32,
    pub v_threshold: f32,
    pub v_reset: f32,
    pub tau_m: f32,
    pub tau_syn: f32,
    pub in_refractory: bool,
    pub t_ref: f32,
    pub t_ref_remaining: f32,
    pub total_spikes: u64,
    pub last_spike_time: f64,
    pub activity_ema: f32,
}

pub struct Connectome {
    adjacency: CsMat<f32>,
    tag_to_idx: HashMap<TagId, usize>,
    idx_to_tag: Vec<TagId>,
    total_events: u64,
}
```

**传播控制细节：**
- 逐跳衰减：`decay = hop_decay^hop`
- 传播门限：`edge_weight >= similarity_gate`
- 收敛条件：窗口内激活数量方差 / 均值 < 阈值
- 全局抑制：活跃神经元数 > max_active 时统一衰减电位

**共现矩阵更新：**
- 共现权重 = 旧值×衰减 + 新增权重
- 仅保留上三角落盘，恢复时对称展开
- 定期剪枝：移除低于 min_weight 的边

---

### 3.3 Thalamus：Tide 算法引擎

**核心职责：**
- 通过 Gram-Schmidt 分解剥离主语义
- 通过投影熵判断意图聚焦程度
- 通过世界观门控调整检索参数
- 通过引力场重塑 query

**关键环节：**
1. **能量分解（MGS）**：避免数值不稳定
2. **投影熵**：输出 FocusLevel
3. **世界观门控**：分类后动态调整参数
4. **弱信号捕获**：在残差空间挖掘被掩盖语义

**实现约束：**
- 分解层数必须限制（默认 5）
- 残差能量阈值要有硬保护

**分解流程细节：**
```
query → 残差 r0
for each layer:
  找到与残差最相似的 tag 向量 t
  projection = proj(r, t)
  residual = r - projection
  记录该层能量占比
  若 residual_energy / initial_energy < threshold → stop
```

**投影熵计算：**
```
projections = [|cos(q, tag_i)|]
probs = projections / sum(projections)
entropy = -Σ p_i ln p_i
normalized = entropy / ln(N)
```

**世界观门控参数调整规则（示例）：**
- Technical：提高相似度阈值、降低扩散 hop
- Emotional：降低阈值、增加 hop、允许弱信号
- Creative：最大化探索、扩大 max_results

**语义引力场细节：**
- 质量 = sqrt(doc_count) × avg_cooccur × recency_factor
- 半径 = 0.3 + 0.1×ln(mass)
- 力 = G × mass / distance^n

---

### 3.4 RetrievalPipeline：全链路检索管线

**管线阶段：**
1. Query Embedding
2. Thalamus 分析（worldview + entropy）
3. Cortex 过召回（引力场修正）
4. Tide 弱信号补充
5. LIF 脉冲扩散补充
6. 语义去重 + 引力坍缩再排序

**重要约束：**
- 所有高级检索模式必须可降级为 Basic
- 主路径最大延迟必须有上限策略（超时回退）

**检索模式定义：**
- Basic：仅向量 Top-K
- Tide：向量 + 能量分解 + 弱信号
- TagMemo：Tide + LIF 脉冲扩散
- Dream：扩大 hop + 结果数 + 低阈值

**结果融合策略：**
- Cortex/WeakSignal/Spike 统一进入候选池
- Dedup 先于重排，防止重复分数放大
- 引力坍缩只调整 score，不改来源标记

**结果来源标识：**
- Vector
- WeakSignal
- SpikeAssociation { hop, pathway }

---

## 四、写入链路与数据生命周期

### 4.1 Ingest 全流程

Seahorse 的写入链路必须与检索链路同等重要。系统不仅要能“想起来”，还必须能稳定地“记进去、改得动、删得掉、可恢复”。

**标准写入流程：**
1. **输入接收**：接收原始文本、文件、对话或结构化记忆对象
2. **预处理**：清洗文本、标准化换行、去除无意义控制字符、计算内容哈希
3. **Chunk 切分**：按 token/语义边界切分为 chunk
4. **Embedding 生成**：调用 EmbeddingProvider 批量生成向量
5. **Tag 提取**：规则 + LLM + 用户显式标签融合
6. **Tag 规范化**：别名归一、同义词映射、类别补全、停用词过滤
7. **SQLite 事务写入**：写 files / chunks / tags / chunk_tags
8. **向量索引更新**：将 chunk embedding 插入 HNSW
9. **Connectome 更新**：基于 chunk 的 tag 集合更新共现矩阵
10. **Neuron State 更新**：初始化缺失神经元，更新使用统计
11. **异步快照**：根据策略刷新 mmap 快照
12. **审计日志**：记录 ingest 结果、耗时、失败原因

**事务策略：**
- `files/chunks/tags/chunk_tags/retrieval_log` 必须在同一 SQLite 事务中提交
- HNSW 与 Connectome 更新可采用“事务提交后异步更新”模式
- 若异步更新失败，必须记录 repair task，保证后台可重建

**失败回滚策略：**
- SQLite 事务失败 → 全量回滚
- 向量索引失败但 SQLite 成功 → 标记 `index_status = pending_repair`
- Connectome 更新失败 → 标记 `connectome_status = pending_repair`
- 系统启动时自动扫描 repair queue

### 4.2 Chunk 切分策略

**切分原则：**
- 优先保持语义完整，其次控制 token 长度
- 默认 chunk 大小：`300~800 tokens`
- 默认 overlap：`50~100 tokens`
- 对话类数据按轮次切分；文档类数据按段落/标题层级切分

**切分模式：**
- `FixedToken`：按固定 token 切分
- `SemanticParagraph`：按段落与标题边界切分
- `DialogueTurn`：按对话轮次切分
- `Custom`：允许上层传入切分器

### 4.3 Update / Forget / Delete 生命周期

**删除策略采用 Tombstone + 延迟压缩：**
- 业务删除不立即重建 HNSW
- 被删除 chunk 标记 `deleted_at` 与 `is_deleted = true`
- 查询阶段跳过 tombstone
- 达到阈值后触发 compaction/rebuild

**删除后的级联行为：**
1. chunk 标记删除
2. chunk_tags 失效
3. HNSW 标记删除节点
4. connectome 不立即反向扣减，采用“时间衰减 + 周期性重建”策略
5. tag 质心在后台重算

**忘记（Forget）模式：**
- `SoftForget`：逻辑删除，可恢复
- `HardForget`：彻底删除 chunk 与索引记录
- `DecayForget`：降低相关边权与访问权重，不立即删除

### 4.4 Reindex / Repair / Rebuild

**触发场景：**
- 索引损坏
- embedding 模型升级
- tombstone 比例过高
- repair queue 积压
- schema / archive version 升级

**重建顺序：**
1. 从 SQLite 扫描有效 chunks
2. 重新生成或加载 embedding
3. 重建 HNSW
4. 重建 tag centroid
5. 重建 connectome
6. 重新归档 mmap

---

## 五、Tag 与 Embedding 体系

### 5.1 Tag 体系设计

Tag 是 Seahorse 的核心语义桥梁，直接连接：
- Cortex 的结果解释
- Synapse 的扩散网络
- Thalamus 的分解与门控

**Tag 来源：**
- 用户显式传入
- 规则提取（正则/关键词/metadata）
- LLM 自动提取
- 离线批处理补标

**Tag 分类建议：**
- `core`：系统核心标签，跨领域复用
- `domain`：专业领域标签
- `temporal`：时间相关标签
- `entity`：人名/组织/地点
- `emotional`：情绪与关系
- `custom`：用户自定义

### 5.2 Tag 规范化

**规范化流程：**
1. trim + 小写归一（必要时保留展示名）
2. 同义词映射
3. 别名表合并
4. 停用 tag 过滤
5. category 推断或补全
6. 去重与 confidence 合并

**需要额外维护的表：**
- `tag_aliases(alias, canonical_tag_id)`
- `tag_synonyms(tag_id, synonym)`
- `tag_stopwords(word)`

### 5.3 Tag 质心（Centroid）

**定义：**
Tag 质心向量为其关联 chunk embedding 的加权均值。

**推荐加权公式：**
```text
centroid(tag) = Σ (embedding_i × confidence_i × recency_i) / Σ (confidence_i × recency_i)
```

**更新策略：**
- 小规模写入：增量更新
- 大规模删除/重构：批量重算
- 默认按时间衰减加权，避免旧记忆长期主导

### 5.4 EmbeddingProvider 抽象

**核心接口要求：**
- `embed(text: &str) -> Vec<f32>`
- `embed_batch(texts: &[String]) -> Vec<Vec<f32>>`
- `dimension() -> usize`
- `model_id() -> String`
- `max_batch_size() -> usize`

**工程约束：**
- 同一 memory namespace 默认只能存在一种主 embedding 维度
- 不同模型升级时必须触发 re-embed 或建立新 index version
- 所有 embedding 必须附带 `model_id` 与 `dimension`

### 5.5 Embedding 缓存与迁移

**缓存：**
- key = `hash(text + model_id)`
- 支持内存 LRU + SQLite 持久缓存
- 批量请求优先合并

**迁移策略：**
- 新模型引入后创建新 index version
- 老版本维持只读，直到 rebuild 完成
- 切换采用“双索引切换”而非原地覆盖

---

## 六、接口契约与结果模型

### 6.1 核心结果对象

```rust
pub struct RecallResultItem {
    pub chunk_id: i64,
    pub vector_id: String,
    pub chunk_text: String,
    pub source_file: Option<String>,
    pub tags: Vec<String>,
    pub score: f32,
    pub source_type: ResultSource,
    pub hop_distance: Option<usize>,
    pub spike_pathway: Option<Vec<String>>,
    pub weak_signal_strength: Option<f32>,
    pub metadata: HashMap<String, String>,
}
```

### 6.2 ResultSource 统一定义

```rust
pub enum ResultSource {
    Vector,
    WeakSignal,
    SpikeAssociation { hop: usize, pathway: Vec<TagId> },
}
```

### 6.3 最终排序公式

```text
final_score =
  w_vector * vector_score +
  w_spike * spike_score +
  w_weak * weak_signal_score +
  w_gravity * gravity_bonus -
  w_dup * duplication_penalty
```

**建议默认权重：**
- Basic：`1.0 / 0 / 0 / 0.05 / 0.2`
- Tide：`0.75 / 0 / 0.2 / 0.1 / 0.2`
- TagMemo：`0.55 / 0.25 / 0.1 / 0.1 / 0.2`
- Dream：`0.4 / 0.3 / 0.15 / 0.15 / 0.15`

### 6.4 Rust Core API

```rust
Engine::open(path)
Engine::ingest_text(input)
Engine::ingest_chunks(input)
Engine::recall(query, mode, params)
Engine::forget(filter)
Engine::dream(options)
Engine::stats()
Engine::rebuild_index()
Engine::compact()
```

### 6.5 REST API

- `POST /ingest`
- `POST /recall`
- `POST /forget`
- `POST /dream`
- `GET /stats`
- `GET /health`
- `POST /admin/rebuild`

### 6.6 错误码体系

- `INVALID_INPUT`
- `EMBEDDING_FAILED`
- `INDEX_UNAVAILABLE`
- `STORAGE_ERROR`
- `TIMEOUT`
- `PARTIAL_RESULT`
- `UNSUPPORTED_MODEL_VERSION`

---

## 七、后台任务与维护策略

### 7.1 Cerebellum 调度模型

**任务类型：**
- RebuildIndex
- RepairPendingIndex
- RecomputeCentroids
- PruneConnectome
- CompactMemory
- DreamRun
- HealthAnalyze

**调度要求：**
- 支持优先级
- 支持重试与指数退避
- 支持幂等任务签名
- 禁止阻塞 recall 主路径

### 7.2 Dream 模式

**Dream 目标：**
- 不是生成幻想文本，而是做“离线联想整合”
- 可选接入 LLM 生成 dream narrative，但默认不开启自动写回

**Dream 流程：**
1. 随机或基于访问热度选取种子
2. 放大 hop 与 max_results
3. 运行深度联想检索
4. 生成候选关联与摘要
5. 默认进入 pending_review，不直接进入主记忆

### 7.3 Memory Compaction

**触发条件：**
- tombstone 比例超阈值
- 高相似 chunk 聚集
- 冷数据比例上升

**压缩原则：**
- 压缩结果必须保留可追溯来源链
- 原始记忆默认保留，可归档到冷存储

---

## 八、配置、安全与多租户

### 8.1 配置系统

建议配置层级：
1. 默认配置
2. 配置文件（TOML）
3. 环境变量覆盖
4. 运行时参数覆盖

**配置域：**
- storage
- embedding
- hnsw
- synapse
- thalamus
- pipeline
- observability
- security

### 8.2 多租户 / Namespace

生产实现建议支持 namespace：
- 每个 namespace 独立逻辑空间
- SQLite 主表附带 `namespace_id`
- HNSW / connectome 支持按 namespace 隔离
- 不同 namespace 不共享 recall 结果

### 8.3 安全要求

- API 层必须鉴权
- 生产环境支持 SQLite 文件级加密或磁盘加密
- 支持 PII 清洗/脱敏写入策略
- 记录审计日志
- Recall 前可选安全过滤，防止恶意记忆被优先召回

---

## 九、测试、评测与迁移策略

### 9.1 测试矩阵

**单元测试：**
- 距离计算
- HNSW 插入/搜索
- LIF 动力学
- Gram-Schmidt 正交性
- 投影熵
- 引力场变形

**集成测试：**
- ingest → recall
- delete → recall
- rebuild → recover
- sqlite + mmap 一致性

**端到端测试：**
- Python SDK → Rust Core → SQLite
- REST → recall → result source correctness

### 9.2 评测指标

- Recall@K
- MRR
- nDCG
- Latency P50/P95/P99
- Cold Start
- Memory Footprint
- Connectome Density
- Active Neuron Percentage

### 9.3 数据集设计

- 合成数据集（可控验证）
- 小规模真实文档集
- 跨域联想测试集
- 弱信号测试集
- 多轮对话长期记忆测试集

### 9.4 迁移与版本兼容

**必须跟踪的版本：**
- schema version
- mmap archive version
- embedding model version
- index format version

**迁移原则：**
- 向后兼容优先
- 不兼容变更走 rebuild
- 大版本切换使用双索引切换

---

## 九、测试、评测与迁移策略

### 9.1 测试矩阵

**单元测试：**
- 距离计算
- HNSW 插入/搜索
- LIF 动力学
- Gram-Schmidt 正交性
- 投影熵
- 引力场变形

**集成测试：**
- ingest → recall
- delete → recall
- rebuild → recover
- sqlite + mmap 一致性

**端到端测试：**
- Python SDK → Rust Core → SQLite
- REST → recall → result source correctness

### 9.2 评测指标

- Recall@K
- MRR
- nDCG
- Latency P50/P95/P99
- Cold Start
- Memory Footprint
- Connectome Density
- Active Neuron Percentage

### 9.3 数据集设计

- 合成数据集（可控验证）
- 小规模真实文档集
- 跨域联想测试集
- 弱信号测试集
- 多轮对话长期记忆测试集

### 9.4 迁移与版本兼容

**必须跟踪的版本：**
- schema version
- mmap archive version
- embedding model version
- index format version

**迁移原则：**
- 向后兼容优先
- 不兼容变更走 rebuild
- 大版本切换使用双索引切换

## 十、参数默认值、范围与调优建议

### 10.1 HNSW 参数

| 参数 | 默认值 | 建议范围 | 说明 |
|---|---:|---:|---|
| `m` | 16 | 8~32 | 每层最大连接数 |
| `m_max_0` | 32 | 16~64 | 底层最大连接数 |
| `ef_construction` | 200 | 100~400 | 构建宽度 |
| `ef_search` | 64 | 32~256 | 查询宽度 |

### 10.2 LIF 参数

| 参数 | 默认值 | 建议范围 | 说明 |
|---|---:|---:|---|
| `tau_m` | 20.0 | 10~50 | 膜时间常数 |
| `tau_syn` | 5.0 | 2~20 | 突触时间常数 |
| `threshold_base` | 1.0 | 0.5~2.0 | 基础阈值 |
| `refractory_period` | 2.0 | 1~10 | 不应期 |
| `hop_decay` | 0.7 | 0.4~0.9 | 逐跳衰减 |
| `similarity_gate` | 0.1 | 0.05~0.5 | 传播门限 |
| `max_active` | 50 | 20~500 | 最大激活神经元 |
| `convergence_threshold` | 0.1 | 0.02~0.2 | 收敛阈值 |
| `min_fire_potential` | 0.8 | 0.5~1.5 | 最小放电电位 |

### 10.3 Tide 参数

| 参数 | 默认值 | 建议范围 | 说明 |
|---|---:|---:|---|
| `max_layers` | 5 | 3~10 | 最大剥离层数 |
| `residual_threshold` | 0.05 | 0.01~0.2 | 残差终止阈值 |
| `min_variance_explained` | 0.01 | 0.005~0.05 | 最小解释方差 |
| `gravity_strength` | 0.2 | 0.05~0.5 | 引力场强度 |
| `gravity_decay_exponent` | 2.0 | 1.5~3.0 | 距离衰减指数 |

### 10.4 调优建议

- **技术知识库场景**：提高 `similarity_gate`，降低 `max_hops`
- **个人长期记忆场景**：降低 `similarity_gate`，提高 `max_hops`
- **创意联想场景**：提高 `gravity_strength` 与 `max_results`
- **生产保守模式**：关闭 Dream 自动写回、限制 weak signal 权重

---

## 十一、异常处理与降级策略矩阵

| 场景 | 检测条件 | 降级行为 | 后续处理 |
|---|---|---|---|
| Embedding 服务超时 | 超过 provider timeout | 返回缓存 embedding 或拒绝 ingest | 写入告警日志 |
| HNSW 不可用 | mmap 校验失败/索引为空 | fallback 到 SQLite 粗筛 + embedding 重算 | 加入 repair queue |
| Connectome 损坏 | 载入失败/维度异常 | 关闭 spike propagation，仅走 Vector/Tide | 后台重建 connectome |
| Worldview 低置信 | top1-top2 差值过低 | 关闭 worldview gate | 记录分类样本 |
| Tide 数值异常 | residual NaN/Inf | 关闭 weak signal，继续 Basic/TagMemo | 记录异常样本 |
| Spike 激活过载 | active > max_active | 立即全局抑制并截断 hop | 记录参数快照 |
| 部分结果失败 | 某阶段失败 | 返回 `PARTIAL_RESULT` | 在 metadata 中标记失效阶段 |

**统一原则：**
- recall 优先可用性
- ingest 优先一致性
- 所有降级必须可观测

 
