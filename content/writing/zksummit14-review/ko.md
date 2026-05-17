---
title: "ZK Summit 14 리뷰: 발표, 주제, 시사점"
date: "2026-05-18"
tags: [review, zksummit, zk, cryptography, event]
description: "ZK Summit 14의 발표, 주요 주제, 시사점을 정리합니다."
language: ko
---

# ZK Summit 14 리뷰: 발표, 주제, 시사점

## Introduction

ZK Summit은 zero-knowledge 생태계의 주요 정기 행사 중 하나다. 2018년부터 이어져 왔고, ZK를 중심으로 활동하는 cryptographer, protocol engineer, infrastructure team, founder, application builder들이 모이는 자리다. Formal proceedings를 갖춘 academic conference라기보다는, 연구와 실제 적용 사례를 함께 다루는 curated one-day summit에 가깝다. Academic cryptography와 실제 시스템 적용 사이의 접점에 놓인 행사이기 때문에, ZK 생태계에서 실제로 무엇이 만들어지고 있고 어떤 문제가 논의되고 있는지 보기 좋은 지점이 된다.

ZK Summit 14는 2026년 5월 7일 로마에서 열렸다. Main stage와 side stage를 합쳐 연구 중심 발표와 응용 중심 발표가 함께 배치된 하루짜리 행사였고, 공식 일정 기준으로 welcome, break, lunch, panel을 제외하면 총 24개의 발표가 있었다. 발표자 구성도 다양했다. Ethereum Foundation, Succinct, Zcash/Project Tachyon, Aztec Labs, Nethermind, Miden, Nokia Bell Labs, Powdr Labs 같은 팀뿐 아니라 EPFL, UCL, NYU, TU Graz, IMDEA, University of Pennsylvania 같은 연구기관과 대학에서도 발표가 있었다.

ZK14에서 가장 두드러진 점은 ZK가 실제 시스템 안으로 들어가면서 어떤 요구와 압력을 받는지가 분명하게 드러났다는 점이었다. Privacy, soundness, verifiability는 더 이상 proof system 레벨에서만 이야기할 수 있는 추상적 속성으로 머물지 않는다. 실제 사용자, 데이터 흐름, 계정 모델, 운영 환경, 구현 실수 속에서도 그 속성이 유지되는지가 중요해졌다. 그중 가장 뚜렷한 변화는 security and correctness의 비중이 크게 올라왔다는 점이고, privacy 역시 단일 application category가 아니라 systems design 문제로 다뤄졌다는 점이었다.

이 글에서는 먼저 24개 발표를 몇 가지 주제로 나눠보고, 그 다음 프로그램에서 반복적으로 보인 흐름을 정리한다.

## Program-Level Theme Table

24개 발표를 다섯 가지 주제로 나누면 아래와 같다.

| 주제 | 발표 수 | 발표 |
|---|---:|---|
| Privacy-preserving systems | 7 | Zcash Tachyon, Merces, client-side validation, zkID, telemetry, ZK-AntiCheat, eDAS |
| Security / correctness | 5 | Poseidon, EF zkEVM Security Sprint, Origin Tags, ZK engineering security, Proof of Seed |
| ZK properties and adjacent primitives | 5 | ZOOK, VEIL, verifiable FHE, witness encryption, resource-sharing permutations |
| Proving infrastructure / zkVMs | 5 | lambda-vm, Autoprecompiles, Miden ACE, sumcheck trade-offs, non-native arithmetic |
| Broader verifiable applications | 2 | qedb, Jolt Atlas |

과거 ZK Summit과 나란히 놓고 보면 ZK14의 차이는 더 분명해진다.

| 회차 | 대표 발표 | ZK14와 비교했을 때 |
|---|---|---|
| ZK11 | Binius, SNARK proving ASICs, 1 Circuit 5 Rollups, MPC-enabled proof markets | Proof system과 infrastructure에 무게중심이 있었고, application experiment는 상대적으로 배경에 있었다. |
| ZK12 | zkVMs vs custom circuits, hardware acceleration, Bitcoin constraints, universal setups, games/AI applications | Performance, usability, deployment constraint 사이의 긴장이 두드러졌다. |
| ZK13 | OpenVM, Lifted FRI, Ligerito, Ligetron ZK Apps | zkVM, proof-system design, developer-facing application이 계속 중요한 축이었다. |

발표 수로는 privacy가 가장 큰 묶음이었지만, 과거 ZK Summit과 비교했을 때 가장 뚜렷한 변화는 security/correctness의 부상이다. 직전 몇 회에서는 새로운 proof system, zkVM, prover performance, hardware, application experiment가 두드러졌다. ZK14에도 그런 흐름은 남아 있었지만, primitive security margin, zkEVM soundness, Fiat-Shamir bug, ZK engineering practice, 장기적인 migration 문제가 명시적인 발표 주제로 올라왔다. Privacy 역시 고립된 end-user application 묶음이라기보다 payment protocol, identity, data availability, telemetry, validation, anti-cheat 전반의 시스템 설계 문제로 등장했다.

## Privacy-Preserving Systems

Privacy 관련 발표는 ZK14에서 가장 큰 묶음이었다. 이 발표들이 공유한 핵심은 privacy를 공개 경계의 설계 문제로 다뤘다는 점이다. 누가 witness를 보는가, 무엇이 공개 정보가 되는가, 누가 proof 생성이나 유효성 검증에 참여하는가, verifier가 실제로 무엇을 알 권리가 있는가가 반복적으로 등장했다.

결제 관련 발표들은 이 질문을 여러 각도에서 보여줬다. Scaling Zcash with Project Tachyon은 shielded payment를 더 큰 규모로 확장하는 문제를 다뤘다. Merces는 MPC와 CoSNARKs를 이용한 private token transfer를 제안했는데, 여기서는 무엇을 숨길지뿐 아니라 누가 proof 생성에 참여하는지도 설계의 일부가 된다. Client-side Validation in Private Payment Protocols는 모든 거래 세부 사항을 전역적으로 공개하는 대신, 유효성 검증의 일부를 결제 참여자 쪽으로 옮기는 접근을 보여줬다. 세 발표의 구조는 다르지만, 같은 제약을 공유한다. 결제 시스템은 충분히 검증 가능해야 하지만, 모든 정보를 모두에게 공개해서는 안 된다.

Identity와 운영 데이터 관련 발표도 같은 질문을 결제 밖으로 확장했다. zkID는 전체 신원을 드러내지 않고 어떤 조건이나 credential을 만족한다는 사실을 증명하는 문제를 다뤘다. Confidential and Verifiable Telemetry는 민감할 수 있는 운영 데이터를 그대로 공개하지 않으면서, 그 데이터에서 나온 주장을 검증 가능하게 만드는 방향을 제시했다. ZK-AntiCheat는 게임 영역에서 비슷한 문제를 다뤘다. 시스템은 사용자 환경이나 행동에 대한 확신을 원하지만, anti-cheat가 전면적인 감시가 되어서는 안 된다.

eDAS는 data availability sampling에 privacy와 compliance 제약을 결합하면서 이 논의를 데이터 인프라 쪽으로 옮겼다. Data availability는 흔히 공개 검증 문제로 이야기되지만, 실제 배포 환경에서는 선택적 공개, access control, 지역별 규제 요건이 함께 들어올 수 있다.

따라서 ZK14의 privacy 흐름은 단순히 "ZK가 데이터를 숨긴다"는 이야기로 정리되지 않는다. 핵심은 공개와 검증 사이의 경계를 설계하는 문제였다. 어떤 시스템에서는 proof가 witness를 숨긴다. 다른 시스템에서는 유효성 검증이 위임되거나 분산되거나 다른 참여자에게 이동한다. 반복된 질문은 시스템이 어디서 데이터를 공개하고, 어디서 검증 가능한 주장만 공개하며, 어디서 신뢰와 가시성을 다른 구조로 옮길 것인가였다.

## Security and Correctness

Security and correctness는 이번 프로그램에서 가장 뚜렷하게 부상한 주제였다. 핵심 질문은 단순히 proof가 검증되는지가 아니었다. 검증된 proof가 실제로 무엇을 보장하는가였다.

검증된 증명이 보장하는 것은 해당 primitive와 protocol, circuit, VM, 구현의 가정 아래에서 실제로 인코딩된 관계가 성립한다는 사실이다. 따라서 ZK security는 여러 층위로 나뉜다. Hash assumption, Fiat-Shamir transcript binding, constraint completeness, VM semantics, witness generation, deployment practice가 모두 시스템이 의도한 내용을 증명하는지에 영향을 준다. ZK14는 이 층위별 보안 문제를 매우 선명하게 보여줬다.

Poseidon 발표는 primitive 레벨의 문제를 다뤘다. Poseidon과 Poseidon2는 proof system 안에서 효율적으로 쓰기 위해 널리 사용되는 hash function이다. 따라서 이들에 대한 algebraic attack과 security margin을 점검하는 일은 단순한 hash function 연구를 넘어선다. ZK-friendly hash가 여러 시스템의 공유 인프라가 되면, 그 보안 가정도 공유 인프라가 된다.

EF zkEVM Security Sprint는 같은 문제를 시스템 레벨에서 보여줬다. zkEVM은 보통 performance, compatibility, real-time proving 관점에서 많이 이야기된다. 하지만 그보다 먼저 필요한 것은 soundness다. 인코딩된 execution semantics가 잘못됐거나 invalid execution이 통과될 수 있다면, 빠른 proving은 잘못된 보장을 더 싸게 만들어낼 뿐이다. 이 발표는 성능 논의보다 soundness 기준을 앞세웠다.

Origin Tags와 A Security Guide to ZK Engineering은 구현 실무 쪽으로 시선을 옮긴다. Fiat-Shamir 변환 오류, transcript binding 실수, 누락된 constraint, witness generation과 실제로 강제되는 relation 사이의 불일치는 모두 같은 실패로 이어질 수 있다. proof는 검증되지만, 의도한 statement를 증명하지 못하는 것이다. Proof of Seed는 장기적인 migration에서도 account ownership이라는 보안 불변식을 어떻게 유지할 것인지의 문제를 더했다. Legacy account ownership을 post-quantum credential에 연결하면서 address 변경이나 UX disruption을 줄이는 것이 핵심이다.

이 발표들을 묶어보면 security는 ZK stack의 부가적인 체크리스트가 아니라 핵심 설계 축이다. 이제 질문은 proof system이 충분히 빠른가에만 있지 않다. 그 proof가 어떤 statement와 입력에 묶여 있는지, 어떤 primitive와 구현 가정에 기대고 있는지, 실제 가치가 걸린 시스템에서 믿을 수 있는지가 중요하다.

## Primitives and Preserved Guarantees

Primitive 관련 발표들도 단순히 더 빠른 proof system을 제안하는 데 머물지 않았다. 여러 발표의 핵심은 ZK와 verifiable computation을 유용하게 만드는 속성, 즉 zero-knowledge, confidentiality, verifiability, computational integrity를 어떻게 유지할 것인가에 있었다.

ZOOK와 VEIL은 이 흐름에 가장 직접적으로 들어맞는다. ZOOK는 constrained interleaved code에 대한 zero-knowledge IOPP를 다뤘고, VEIL은 hash-based multilinear proof system에 lightweight zero-knowledge를 붙이는 문제를 다뤘다. 둘 다 기술적인 proof-system 발표지만, 핵심은 효율적인 construction 안에서도 zero-knowledge 성질을 어떻게 유지할 것인가에 있다.

Making verifiable FHE practical은 비슷한 문제를 다른 방향에서 다룬다. FHE는 암호화된 데이터 위에서 계산할 수 있게 하지만, 위탁된 계산이 올바르게 수행됐는지는 별개의 보장이다. Verifiable FHE의 포인트는 데이터를 기밀로 유지하는 것뿐 아니라, 그 계산 결과를 검증 가능하게 만드는 데 있다.

Witness encryption과 resource-sharing permutations는 단순히 ZK proof-system 발표라고 보기 어렵다. 오히려 인접한 cryptographic primitive와 computational integrity 연구에 가깝다. 하지만 두 발표도 같은 질문을 다른 방향으로 넓힌다. 시스템이 조합되고 최적화되는 과정에서도 의도한 보장이 유지되게 하려면 무엇이 필요한가? 따라서 이 묶음은 하나의 primitive category라기보다, ZK 시스템이 기대는 보장들이 어떻게 구성되고 유지되는가에 대한 논의로 보는 편이 자연스럽다.

## Proving Infrastructure and zkVMs

Proving infrastructure와 zkVM 관련 발표도 여전히 중요한 축이었다. 다만 이번 프로그램에서 이 주제는 단순히 더 빠르게 증명하는 방법이라기보다, 실제 application이 요구하는 보장을 감당할 수 있는 형태로 infrastructure가 특화되고 있음을 보여줬다.

lambda-vm은 작고 성능 지향적인 zkVM을 제안했다. Nondeterministic Autoprecompiles는 zkVM 안에서 비싼 연산을 더 효율적으로 처리하는 방향을 다뤘다. Miden's ACE chiplet은 Miden VM 내부에서 arithmetic circuit evaluation을 효율적으로 처리하기 위한 구성 요소였다. 이 발표들을 함께 보면 zkVM 논의가 "일반 프로그램 실행을 증명할 수 있는가"를 넘어, 어떤 연산이 비용을 지배하는가, 어떤 부분에 전용 가속이 필요한가, VM 내부 구조를 어디까지 활용할 수 있는가로 이동하고 있음을 알 수 있다.

Time-Space Trade-Offs for Sumcheck와 Efficient Non-Native Arithmetic from SNARKs for Integers는 zkVM 자체에만 묶인 발표는 아니지만, 증명 인프라 관점에서 중요하다. Sumcheck의 시간과 메모리 trade-off는 대규모 proof system의 prover cost와 직접 연결된다. Non-native arithmetic은 기존 cryptographic primitive, 다른 field, 다른 체인의 계산을 ZK 안에서 다룰 때 반복적으로 등장하는 병목이다. 낮은 층위의 주제처럼 보이지만, 상위의 privacy와 verifiability 시스템이 실제로 배포될 수 있는지를 좌우한다.

따라서 infrastructure는 application과 분리된 별도 층위가 아니다. 실제 application이 병목을 드러낸다. 어떤 application이 특정 hash, field, VM instruction, cross-chain verification path를 필요로 한다면, infrastructure 작업은 그 사용 사례가 실제로 가능한지 아니면 이론적으로만 표현 가능한지를 결정한다.

## Broader Verifiable Applications

Privacy나 payment에 직접 묶이지 않는 verifiable application도 있었다. 대표적으로 qedb와 Jolt Atlas가 그렇다. 둘 다 정보를 숨기는 문제보다는, 외부 시스템이 수행한 계산이나 질의가 정해진 상태와 절차에 따라 올바르게 수행됐는지를 검증하는 문제에 더 가깝다.

qedb는 SNARK 없이 expressive and modular verifiable database를 만드는 방향을 다뤘다. 여기서 보장은 조심해서 읽어야 한다. 데이터베이스 안의 내용이 외부적으로 참이라는 뜻이 아니라, 질의 결과가 authenticated 혹은 committed database state와 일치한다는 뜻이다. 흥미로운 지점은 모든 verifiable system을 general-purpose SNARK나 zkVM에 넣을 필요는 없다는 것이다. Database라는 도메인에 맞는 검증 가능 구조가 더 나은 설계일 수 있다.

Jolt Atlas는 verifiable inference를 다뤘다. 여기서도 보장되는 것은 computational integrity이지 semantic truth가 아니다. Proof는 특정 model과 input에 대해 정해진 inference computation이 주장한 대로 실행됐음을 보일 수 있다. 하지만 model이 정확한지, 공정한지, 유용한지까지 증명하지는 않는다. Lookup argument를 활용한 inference proof는 넓게 보면 zkML 흐름에 속하지만, 실질적인 가치는 외부 계산을 사후 검증 가능한 대상으로 만드는 데 있다.

qedb와 Jolt Atlas를 함께 보면, 검증 가능성은 blockchain이나 private payment를 넘어 database와 ML inference로 확장되고 있었다. Telemetry와 validation 관련 발표에서도 비슷한 흐름이 보였다. 점점 더 많은 시스템이 모든 신뢰를 operator에게 넘기지 않으면서도, 외부 계산을 검증 가능한 형태로 만들고 싶어 한다.

## Takeaways

전체적으로 보면 ZK14는 특정 proof system이나 application category 하나가 중심을 차지한 행사라기보다, ZK가 여러 실제 시스템 요구 속에서 재정의되고 있음을 보여준 자리였다. Privacy, soundness, verifiability는 proof system 레벨의 추상적인 속성만으로는 충분하지 않다. 실제 사용자, 데이터 흐름, 계정 모델, 운영 환경, 구현 실수 속에서도 그 보장이 유지되는지가 중요해졌다.

1. Security and correctness는 이제 ZK의 독립적인 핵심 주제가 됐다. ZK14에서 가장 뚜렷한 변화도 이 지점이었다. 검증된 proof가 곧 의도한 보장을 의미하지는 않는다. 실제로 인코딩된 관계, 사용하는 primitive, Fiat-Shamir transcript, constraint, VM semantics, witness generation, 배포 방식이 모두 맞아야 proof가 시스템이 원하는 statement를 증명한다.

2. Privacy는 공개 경계를 설계하는 문제로 이동하고 있다. 이번 privacy 발표들은 데이터를 숨기는 방식 자체보다, 누가 witness를 보고 무엇이 공개 정보가 되며 누가 proof 생성과 유효성 검증에 참여하는지를 다뤘다. Payment, identity, telemetry, anti-cheat, data availability에서 반복된 질문은 검증 가능성을 유지하면서 어디까지 공개할 것인가였다.

3. Primitive와 engineering 관련 발표 상당수는 속도 개선보다 보장 유지에 초점이 있었다. ZOOK와 VEIL은 효율적인 proof-system construction 안에서 zero-knowledge 성질을 유지하는 문제를 다뤘고, Verifiable FHE는 confidentiality와 outsourced computation correctness를 함께 묶었다. 중요한 지표는 prover time만이 아니라, 최적화와 모듈화, 실제 배포 이후에도 어떤 보장이 남는가다.

4. zkVM과 proving infrastructure는 범용성 중심에서 특화 중심으로 옮겨 가고 있다. 실제 application은 특정 hash, field, VM instruction, non-native arithmetic, 메모리 제약 같은 병목을 드러낸다. Infrastructure 작업은 그 병목을 줄이는 동시에 상위 시스템의 privacy, soundness, verifiability 보장을 유지하게 만드는 역할을 한다.

5. 검증 가능성은 crypto-native 영역을 넘어가고 있다. qedb와 Jolt Atlas는 database와 ML inference를 가리키고, telemetry와 validation 관련 발표들은 운영 시스템 쪽을 가리킨다. 여기서 보장되는 것은 semantic truth가 아니라 computational integrity다. 데이터가 외부적으로 참인지, 모델 출력이 좋은지까지 증명하지는 않지만, 외부 계산을 검증 가능한 대상으로 만든다.

ZK14가 보여준 것은 ZK가 하나의 큰 방향으로 수렴하고 있다는 그림이 아니었다. 오히려 ZK가 payment, identity, zkVM, database, ML, telemetry처럼 서로 다른 시스템 안으로 들어가면서, 각 맥락마다 다른 보장을 요구받고 있다는 점이었다.

다음 과제는 proof를 더 빠르게 만드는 데서 끝나지 않는다. 무엇을 숨기고, 무엇을 검증하고, 어떤 실패를 막아야 하는지 먼저 명확히 정해야 한다. 그리고 그 보장이 circuit, protocol, VM, application, operation을 지나 실제 시스템 안에서도 흐트러지지 않게 유지되어야 한다.

## Appendix: Talk Inventory

| 발표 | 주제 | 문제 영역 | 시사점 |
|---|---|---|---|
| [ZOOK: Zero-Knowledge IOPPs for Constrained Interleaved Codes](https://eprint.iacr.org/2026/391) | Primitives | Proof-system construction | Efficiency gain은 proof layer의 zero-knowledge 보장을 잃지 않을 때 의미가 있다. |
| [VEIL: Lightweight Zero-Knowledge for Hash-Based Multilinear Proof Systems](https://eprint.iacr.org/2026/683) | Primitives | Hash-based proof systems | Zero-knowledge는 전체 proof system rewrite가 아니라 별도 layer로 추가될 수도 있다. |
| [Seven years in Poseidon](https://www.poseidon-initiative.info/) | Security / correctness | Primitive security margins | 공유 ZK-friendly primitive에는 일회성 확신보다 지속적인 공개 cryptanalysis가 필요하다. |
| [Scaling Zcash with Project Tachyon](https://seanbowe.com/blog/tachyon-scaling-zcash-oblivious-synchronization/) | Privacy-preserving systems | Shielded payment scaling | Privacy를 scale하려면 proof 성능뿐 아니라 synchronization architecture가 필요하다. |
| [qedb: Expressive and Modular Verifiable Databases (without SNARKs)](https://eprint.iacr.org/2025/1408) | Broader verifiable applications | Verifiable databases | 모든 verifiable system을 general-purpose SNARK나 zkVM으로 표현할 필요는 없다. |
| [zkID](https://pse.dev/blog/revocation-in-zkid-merkle-tree-based-approaches) | Privacy-preserving systems | Identity and credentials | Privacy-preserving identity에는 selective disclosure뿐 아니라 revocation과 credential lifecycle이 필요하다. |
| [EF zkEVM Security Sprint](https://zkevm.ethereum.foundation/blog/cryptography-research-update) | Security / correctness | zkEVM soundness | 빠른 zkEVM도 execution semantics가 sound하고 auditable해야 의미가 있다. |
| [lambda-vm](https://blog.alignedlayer.com/aligned-monthly-recap-december-2025/) | Proving infrastructure / zkVMs | Minimal zkVM design | Minimal zkVM design은 performance만큼이나 auditability를 위한 선택이기도 하다. |
| [Merces](https://eprint.iacr.org/2026/850) | Privacy-preserving systems | Private token transfer | Private token system은 무엇을 숨길지뿐 아니라 누가 계산하고 증명할지도 설계해야 한다. |
| [Nondeterministic Autoprecompiles](https://powdr.org/blog/accelerating-ethereum-with-autoprecompiles) | Proving infrastructure / zkVMs | zkVM acceleration | zkVM performance 작업은 일반 실행에서 bottleneck의 자동 특화로 이동하고 있다. |
| [Proof of Seed](https://www.soundness.xyz/blog/mpc-wallets-the-post-quantum-migration) | Security / correctness | Post-quantum migration | Cryptographic migration은 기존 secret을 노출하지 않으면서 ownership continuity를 유지해야 한다. |
| Origin Tags | Security / correctness | Fiat-Shamir bugs | ZK security failure는 깨진 primitive보다 누락된 binding에서 자주 생긴다. |
| [Making verifiable FHE practical](https://eprint.iacr.org/2025/286) | Primitives | FHE plus verifiability | Confidential computation은 위탁된 결과까지 검증 가능해야 완성된다. |
| [Witness Encryption from Arithmetic Affine Determinant Programs](https://eprint.iacr.org/2026/175) | Primitives | Witness encryption | Witness encryption은 access를 key 보유가 아니라 statement 증명에 묶을 수 있다. |
| [eDAS](https://eprint.iacr.org/2026/325) | Privacy-preserving systems | Data availability and compliance | Public availability check에도 privacy와 regulatory boundary가 필요할 수 있다. |
| Client-side Validation in Private Payment Protocols | Privacy-preserving systems | Private payment validation | Private payment는 validation을 global consensus가 아니라 참여자 쪽으로 옮길 수 있다. |
| [Jolt Atlas](https://arxiv.org/abs/2602.17452) | Broader verifiable applications | Verifiable ML inference | Verifiable inference는 generic execution보다 domain-specific proving 쪽으로 이동하고 있다. |
| [Time-Space Trade-Offs for Sumcheck](https://eprint.iacr.org/2025/1473) | Proving infrastructure / zkVMs | Prover cost | 배포 가능한 prover는 asymptotic time만큼이나 memory constraint에 좌우된다. |
| A Security Guide to ZK Engineering | Security / correctness | Engineering failures | ZK system은 구현이 의도와 다른 보장을 encode할 때 실패한다. |
| [ZK-AntiCheat](https://devfolio.co/projects/zkanticheat-74ee) | Privacy-preserving systems | Gaming anti-cheat | ZK는 전면 inspection을 실제 필요한 property attestation으로 대체할 수 있다. |
| Confidential and Verifiable Telemetry | Privacy-preserving systems | Operational data | Operational observability는 raw measurement data를 공개하지 않고도 verifiable할 수 있다. |
| [Miden's ACE chiplet](https://0xmiden.github.io/air-script/) | Proving infrastructure / zkVMs | VM-specific acceleration | Recursive proving은 VM design을 specialized internal component 쪽으로 이동시킨다. |
| Efficient Non-Native Arithmetic from SNARKs for Integers | Proving infrastructure / zkVMs | Non-native arithmetic | Interoperability cost는 native field가 다루기 어려운 arithmetic에서 자주 드러난다. |
| Resource-Sharing Permutations for Computational Integrity | Primitives | Computational integrity | Efficiency-oriented primitive design에도 명시적인 integrity accounting이 필요하다. |

출처:

- [ZK Summit 14 행사 페이지](https://www.zksummit.com/)
- [ZK Summit 13 행사 페이지](https://zeroknowledge.fm/zksummit13/)
- [ZK Summit 12 행사 페이지](https://zeroknowledge.fm/zksummit12/)
- [ZK Summit 11 행사 페이지](https://zeroknowledge.fm/the-zero-knowledge-summit-11/)
- [ZK Summit 11-14 영상 playlist](https://www.youtube.com/watch?list=PLj80z0cJm8QFy2umHqu77a8dbZSqpSH54&v=nrfueiRDqL4)
