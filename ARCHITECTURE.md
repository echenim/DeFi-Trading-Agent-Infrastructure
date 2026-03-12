# DESIGN.md

Agent-Based DeFi Trading Infrastructure

This document describes the architecture and system design of the
agent-based DeFi trading infrastructure.

------------------------------------------------------------------------

# 1. System Overview

The platform monitors blockchain mempool activity, detects market
opportunities, evaluates trading strategies, validates risk constraints,
and executes transactions on-chain.

The architecture uses independent Rust agents communicating through
asynchronous messaging.

Design priorities:

- Low‑latency event processing
- Deterministic transaction execution
- Modular strategy experimentation
- Fault tolerance under unreliable RPC conditions
- Separation of strategy logic from execution infrastructure

------------------------------------------------------------------------

# 2. Layered Architecture

``` mermaid
flowchart TB

subgraph Data Sources
    A[Blockchain Mempool]
    B[DEX Market State]
end

subgraph Ingestion Layer
    C[Market Data Agent]
end

subgraph Strategy Layer
    D[Arbitrage Strategy]
    E[Liquidation Strategy]
    F[Yield Strategy]
end

subgraph Risk Layer
    G[Risk Enforcement Agent]
end

subgraph Execution Layer
    H[Execution Agent]
end

subgraph External Systems
    I[Ethereum RPC]
    J[DEX Contracts]
end

A --> C
B --> C
C --> D
C --> E
C --> F
D --> G
E --> G
F --> G
G --> H
H --> I
I --> J
```

------------------------------------------------------------------------

# 3. Agent Communication Architecture

``` mermaid
flowchart LR

subgraph Agent System
    MD[Market Data Agent]
    ST[Strategy Agents]
    RK[Risk Agent]
    EX[Execution Agent]
end

MQ[Async Message Bus]

MD --> MQ
MQ --> ST
ST --> MQ
MQ --> RK
RK --> MQ
MQ --> EX
```

Agents communicate through an asynchronous message bus enabling loose
coupling and independent scaling.

------------------------------------------------------------------------

# 4. Runtime Trading Flow

``` mermaid
sequenceDiagram

participant M as Market Data Agent
participant S as Strategy Agent
participant R as Risk Agent
participant E as Execution Agent
participant B as Blockchain

M->>S: Market Signal
S->>R: Trade Intent
R->>E: Validated Trade
E->>B: Submit Transaction
B-->>E: Transaction Hash
E-->>S: Execution Result
```

This diagram shows how a trading signal moves through the system at
runtime.

------------------------------------------------------------------------

# 5. Execution Pipeline

``` mermaid
flowchart TD

A[Trade Intent]
B[Transaction Builder]
C[Gas Estimator]
D[Nonce Manager]
E[Transaction Signer]
F[Broadcast Transaction]

A --> B
B --> C
C --> D
D --> E
E --> F
```

Execution must remain deterministic to prevent nonce conflicts and
transaction duplication.

------------------------------------------------------------------------

# 6. Strategy Plugin Architecture

``` mermaid
flowchart LR

MS[Market Signal]

subgraph Strategy Plugins
    S1[Arbitrage Strategy]
    S2[Liquidation Strategy]
    S3[Yield Strategy]
end

R[Risk Validation]

MS --> S1
MS --> S2
MS --> S3

S1 --> R
S2 --> R
S3 --> R
```

Strategies are isolated from core infrastructure using a plugin system.

------------------------------------------------------------------------

# 7. Failure Handling

``` mermaid
flowchart TD

A[Execute Trade]
B{RPC Failure?}

A --> B

B -- Yes --> C[Retry Logic]
C --> D{Retry Limit?}

D -- Exceeded --> E[Circuit Breaker]
D -- Recovered --> F[Continue Execution]

B -- No --> F
```

Distributed systems assume failure. Retry logic and circuit breakers
protect execution pipelines.

------------------------------------------------------------------------

# 8. Deployment Architecture

``` mermaid
flowchart LR

subgraph Trading Node
    A[Market Data Agent]
    B[Strategy Agents]
    C[Risk Agent]
    D[Execution Agent]
end

subgraph Messaging
    E[Async Message Bus]
end

subgraph External
    F[RPC Provider]
    G[Ethereum Network]
end

A --> E
B --> E
C --> E
D --> E

E --> F
F --> G
```

------------------------------------------------------------------------

# 9. Repository Structure

```plaintext
defi-trading-agents/
│
├── Cargo.toml                # Workspace manifest
├── Cargo.lock
├── README.md
├── DESIGN.md
├── BENCHMARKS.md
│
├── crates/                   # Internal Rust crates
│   │
│   ├── market-data-agent/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   ├── strategy-agent/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       └── strategies/
│   │           ├── arbitrage.rs
│   │           ├── liquidation.rs
│   │           └── yield.rs
│   │
│   ├── risk-agent/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   ├── execution-agent/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   ├── messaging/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   │
│   └── common/
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs
│
├── scripts/
│   ├── run_local.sh
│   └── benchmark.sh
│
├── configs/
│   ├── trading.toml
│   └── rpc.toml
│
└── docs/
    ├── architecture.md
    └── diagrams.md
```

# 10. Design Principles

**Single Responsibility Agents**\
Each agent performs a single task.

**Deterministic Execution**\
Ensures consistent transaction ordering.

**Loose Coupling**\
Agents communicate via messaging rather than shared state.

**Strategy Isolation**\
Strategy plugins evolve independently from infrastructure.
