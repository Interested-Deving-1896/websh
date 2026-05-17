---
title: "ZK Summit 14 Review: Talks, Themes, and Takeaways"
date: "2026-05-18"
tags: [review, zksummit, zk, cryptography, event]
description: "A program-level review of ZK Summit 14, covering talks, themes, and takeaways."
language: en
---

# ZK Summit 14 Review: Talks, Themes, and Takeaways

## Introduction

ZK Summit is one of the main recurring gatherings for the zero-knowledge ecosystem. It has been running since 2018, bringing together cryptographers, protocol engineers, infrastructure teams, founders, and application builders working around ZK. It is not an academic conference in the formal proceedings sense; it is a curated one-day summit built around research- and application-oriented talks, sitting at the boundary between academic cryptography and deployed systems. That makes it a useful place to see what people in the ZK ecosystem are actively building and debating.

ZK Summit 14 took place in Rome on May 7, 2026. It was a one-day event with a mix of research- and application-oriented talks across the main stage and side stage. The official schedule listed 24 talks, excluding the welcome remarks, breaks, lunch, and panel. The speaker lineup was also broad, including teams such as Ethereum Foundation, Succinct, Zcash/Project Tachyon, Aztec Labs, Nethermind, Miden, Nokia Bell Labs, and Powdr Labs, alongside researchers from institutions such as EPFL, UCL, NYU, TU Graz, IMDEA, and the University of Pennsylvania.

What stood out at ZK14 was how clearly it showed the pressures ZK faces as it moves into real systems. Privacy, soundness, and verifiability are no longer properties that can be treated only at the proof-system layer. They have to hold across users, data flows, account models, operational environments, and implementation mistakes. The clearest shift was the increased weight of security and correctness, while privacy appeared less as a single application category and more as a systems design problem.

This review first maps the 24 talks into broad themes, then looks at the program through that lens.

## Program-Level Theme Table

I grouped the 24 talks into five themes:

| Theme | Count | Talks |
|---|---:|---|
| Privacy-preserving systems | 7 | Zcash Tachyon, Merces, client-side validation, zkID, telemetry, ZK-AntiCheat, eDAS |
| Security / correctness | 5 | Poseidon, EF zkEVM Security Sprint, Origin Tags, ZK engineering security, Proof of Seed |
| ZK properties and adjacent primitives | 5 | ZOOK, VEIL, verifiable FHE, witness encryption, resource-sharing permutations |
| Proving infrastructure / zkVMs | 5 | lambda-vm, Autoprecompiles, Miden ACE, sumcheck trade-offs, non-native arithmetic |
| Broader verifiable applications | 2 | qedb, Jolt Atlas |

Against recent ZK Summit editions, the shift in ZK14 becomes clearer:

| Edition | Representative talks | What stood out relative to ZK14 |
|---|---|---|
| ZK11 | Binius, SNARK proving ASICs, 1 Circuit 5 Rollups, MPC-enabled proof markets | Strong proof-system and infrastructure energy, with application experiments in the background. |
| ZK12 | zkVMs vs custom circuits, hardware acceleration, Bitcoin constraints, universal setups, games/AI applications | The major tension was performance, usability, and deployment constraints. |
| ZK13 | OpenVM, Lifted FRI, Ligerito, Ligetron ZK Apps | zkVMs, proof-system design, and developer-facing application work remained prominent. |

That is the main shift I would highlight. Earlier recent programs were visibly shaped by new proof systems, zkVMs, prover performance, hardware, and application experiments. ZK14 still had all of that, but security/correctness appeared with unusual weight: primitive security margins, zkEVM soundness, Fiat-Shamir bugs, ZK engineering practice, and long-term migration all appeared as explicit topics. Privacy also appeared less as a set of isolated end-user applications and more as a systems design concern across payment protocols, identity, data availability, telemetry, validation, and anti-cheat.

## Privacy-Preserving Systems

Privacy-related talks formed the largest cluster in ZK14. The interesting part was not simply that there were many privacy applications. The more important pattern was that these talks treated privacy as a question of disclosure boundaries: who sees the witness, what becomes public, who participates in proof generation or validation, and what the verifier is actually entitled to learn.

The payment talks showed this from several angles. Scaling Zcash with Project Tachyon was about making shielded payments work at larger scale. Merces proposed private token transfers using MPC and CoSNARKs, which changes not just what is hidden but who participates in producing the proof. Client-side Validation in Private Payment Protocols moved part of validation closer to the payment participants instead of making every transaction detail globally visible. These are not the same design, but they share the same pressure: payment systems need enough information to validate, while exposing as little as possible to everyone else.

Identity and operational-data talks extended the same question beyond payments. zkID addressed the problem of proving eligibility or credential possession without revealing a full identity. Confidential and Verifiable Telemetry applied a similar idea to operational measurements: raw telemetry can be sensitive, but claims derived from it may still need to be checked. ZK-AntiCheat brought the pattern into gaming, where a system may want assurance about a user's environment or behavior without turning anti-cheat into full inspection.

eDAS moved the privacy discussion into data infrastructure by adding privacy and compliance constraints to data availability sampling. That matters because data availability is often discussed as a public-verification problem, while real deployments may still need selective disclosure, access control, or jurisdiction-specific constraints.

So the privacy theme at ZK14 was broader than "ZK hides the data." It was about designing the boundary between disclosure and verification. In some systems, the proof hides the witness. In others, validation is delegated, distributed, or moved to a different participant. The recurring question was where a system should reveal data, where it should reveal only a claim, and where it should move trust or visibility to a different part of the architecture.

## Security and Correctness

Security and correctness were the most distinctive themes in the program. The central question was not just whether a proof verifies. It was what a verified proof actually establishes.

A verified proof only establishes the relation that was encoded, under the assumptions of the primitive, protocol, circuit, VM, and implementation. That makes ZK security layered. Hash assumptions, Fiat-Shamir transcript binding, constraint completeness, VM semantics, witness generation, and deployment practice can all affect whether a system proves what its designers think it proves. ZK14 made that layered security problem unusually visible.

The Poseidon talk addressed the primitive layer. Poseidon and Poseidon2 are widely used because they are efficient inside proof systems, so a survey of algebraic attacks and security margins matters beyond hash-function research. If a ZK-friendly hash becomes shared infrastructure, its assumptions become shared infrastructure too.

The EF zkEVM Security Sprint addressed the system layer. zkEVM work is often discussed in terms of performance, compatibility, and real-time proving, but soundness is the more basic requirement. If the encoded semantics are wrong, or if the system admits an invalid execution, faster proving only makes the wrong guarantee cheaper to produce. The talk put a security bar in front of the performance story.

Origin Tags and A Security Guide to ZK Engineering moved the discussion into implementation practice. Fiat-Shamir bugs, transcript-binding mistakes, missing constraints, and mismatches between witness generation and enforced relations can all produce the same practical failure: the proof verifies, but it is not proving the intended statement. Proof of Seed added a longer-term migration angle by asking whether ZK can help bind legacy account ownership to post-quantum credentials while minimizing address or UX disruption.

Taken together, these talks made security a first-class part of the ZK stack. The focus was not only on whether proof systems can be fast enough, but on whether the systems built around them can be trusted when they carry real assumptions, real implementations, and real value.

## Primitives and Preserved Guarantees

The primitive-focused talks did not read simply as proposals for faster proof systems. Several of them were about preserving the properties that make ZK and verifiable computation useful in the first place: zero-knowledge, confidentiality, verifiability, and computational integrity.

ZOOK and VEIL fit this most directly. ZOOK covered zero-knowledge IOPPs for constrained interleaved codes, while VEIL focused on lightweight zero-knowledge for hash-based multilinear proof systems. Both are technical proof-system talks, but the relevant thread here is how to keep zero-knowledge available inside efficient proof-system constructions.

Making verifiable FHE practical sat close to the same problem but from another direction. FHE lets someone compute over encrypted data, while verifiability asks whether the outsourced computation was carried out correctly. The point is not only to keep data confidential, but to make the resulting computation checkable.

Witness encryption and resource-sharing permutations are not simply "ZK proof-system talks." They are better read as adjacent cryptographic and computational-integrity work. They broaden the same question: how do we construct systems where the intended guarantee survives the way the system is composed? In that sense, this section is less about one primitive category and more about the guarantees that ZK systems depend on.

## Proving Infrastructure and zkVMs

Proving infrastructure and zkVM talks were still an important part of the program. They did not dominate the schedule, but they showed that infrastructure work is moving from generality toward specialization.

lambda-vm proposed a minimalistic and performant zkVM. Nondeterministic Autoprecompiles looked at making expensive operations inside zkVMs more efficient. Miden's ACE chiplet focused on efficient arithmetic circuit evaluation inside the Miden VM. Together, these talks point past the basic idea of proving general program execution and toward the internal design questions that determine cost: which operations dominate, which parts need dedicated acceleration, and where VM-specific structure can be exploited.

Time-Space Trade-Offs for Sumcheck and Efficient Non-Native Arithmetic from SNARKs for Integers were not strictly zkVM talks, but they matter for proving infrastructure. Sumcheck time and memory trade-offs directly affect prover cost in large proof systems. Non-native arithmetic is a recurring bottleneck when ZK systems need to handle existing cryptographic primitives, other fields, or computations from other chains. These are low-level topics, but they shape whether higher-level privacy and verifiability systems can be deployed comfortably.

That makes infrastructure less separate from applications than it may first appear. Applications define the pressure points. If an application needs a particular hash, field, VM instruction, or cross-chain verification path, infrastructure work decides whether that use case is practical or just theoretically expressible.

## Broader Verifiable Applications

There were also verifiable applications that were not primarily framed around privacy or payments. qedb and Jolt Atlas stood out here. Both were less about hiding information and more about checking that an external computation or query was performed according to a specified state and procedure.

qedb focused on expressive and modular verifiable databases without SNARKs. The guarantee should be read carefully: it is not that the database content is externally true, but that a query result is consistent with an authenticated or committed database state. The interesting design point is that not every verifiable system needs to be pushed into a general-purpose SNARK or zkVM. A database-specific verifiable structure may be the better fit.

Jolt Atlas focused on verifiable inference. Here too, the guarantee is computational integrity, not semantic truth. A proof can show that a specified inference computation ran as claimed over a given model and input. It does not show that the model is accurate, fair, or useful. Using lookup arguments for this kind of inference proof fits into the broader zkML direction, but the practical value comes from making external computation auditable.

Taken together, qedb and Jolt Atlas show verifiability moving beyond blockchain and private payment settings into databases and machine learning inference. The same pattern also appeared in telemetry and validation-oriented talks: systems increasingly want external computation to be checkable without moving all trust to the operator.

## Takeaways

Taken together, ZK14 did not read like a program organized around one breakthrough proof system or one dominant application category. It read more like a snapshot of a field under deployment pressure. Privacy, soundness, and verifiability are no longer abstract properties that can be discussed only at the proof-system layer; they have to survive contact with real systems, users, data flows, account models, and implementation mistakes.

1. Security and correctness have become central ZK topics in their own right. A verified proof only establishes the relation that was actually encoded, under the assumptions of the primitive, protocol, circuit, VM, and implementation. That makes the security question layered: hash assumptions, Fiat-Shamir transcript binding, constraint completeness, VM semantics, witness generation, and deployment practice all matter. This was the clearest change in emphasis at ZK14.

2. Privacy is increasingly a disclosure-boundary design problem. The privacy talks were not all simple stories where "ZK hides the data." Some moved validation closer to participants, some combined ZK with MPC, some dealt with telemetry or anti-cheat settings, and some added privacy and compliance constraints to data infrastructure. The shared question was who sees the witness, what becomes public, who helps produce the proof, and what the verifier is actually entitled to learn.

3. The primitive and engineering work was often about preserving guarantees rather than only improving speed. ZOOK and VEIL were about keeping zero-knowledge available inside efficient proof-system constructions. Verifiable FHE connected confidentiality with correctness of outsourced computation. Other primitive-oriented talks sat nearby, asking how the cryptographic assumptions that ZK systems depend on can be composed and maintained. The useful metric is not just prover time; it is which guarantees remain intact after the system is optimized, modularized, or deployed.

4. zkVM and proving-infrastructure work is moving from generality toward specialization. The important questions are no longer only whether arbitrary programs can be proven, but which operations dominate cost, which parts need dedicated acceleration, how memory and time trade off, and how non-native arithmetic or VM-specific components shape the practical limits of the system. That makes infrastructure less separate from applications than it may first appear: the applications define the pressure points.

5. Verifiability is moving beyond crypto-native settings. qedb and Jolt Atlas point toward databases and ML inference, while telemetry and validation-oriented talks point toward operational systems. The guarantee here is computational integrity: a query, measurement, or inference was evaluated according to a committed specification and state. It does not automatically prove that the underlying data is true or that a model output is good, but it does make external computation auditable in a way that more systems may want.

The overall takeaway is not that ZK is converging on a single direction. ZK14 showed the opposite: as ZK moves into payments, identity, zkVMs, databases, ML, and telemetry, each setting asks for a different mix of guarantees.

The next phase is not only about making proofs faster. It is about deciding what should remain private, what should be verifiable, and which failures the system must rule out. Those guarantees then have to survive the path from circuit and protocol design through VMs, applications, and operations.

## Appendix: Talk Inventory

| Talk | Primary theme | Problem area | Takeaway |
|---|---|---|---|
| [ZOOK: Zero-Knowledge IOPPs for Constrained Interleaved Codes](https://eprint.iacr.org/2026/391) | Primitives | Proof-system construction | Efficiency gains are incomplete if the proof layer loses zero-knowledge. |
| [VEIL: Lightweight Zero-Knowledge for Hash-Based Multilinear Proof Systems](https://eprint.iacr.org/2026/683) | Primitives | Hash-based proof systems | Zero-knowledge can sometimes be added as a layer, not a full proof-system rewrite. |
| [Seven years in Poseidon](https://www.poseidon-initiative.info/) | Security / correctness | Primitive security margins | Shared ZK-friendly primitives need ongoing public cryptanalysis, not one-time confidence. |
| [Scaling Zcash with Project Tachyon](https://seanbowe.com/blog/tachyon-scaling-zcash-oblivious-synchronization/) | Privacy-preserving systems | Shielded payment scaling | Privacy at scale depends on synchronization architecture, not only proof performance. |
| [qedb: Expressive and Modular Verifiable Databases (without SNARKs)](https://eprint.iacr.org/2025/1408) | Broader verifiable applications | Verifiable databases | Not every verifiable system needs a general-purpose SNARK or zkVM. |
| [zkID](https://pse.dev/blog/revocation-in-zkid-merkle-tree-based-approaches) | Privacy-preserving systems | Identity and credentials | Privacy-preserving identity needs revocation and credential lifecycle design, not just selective disclosure. |
| [EF zkEVM Security Sprint](https://zkevm.ethereum.foundation/blog/cryptography-research-update) | Security / correctness | zkEVM soundness | A fast zkEVM only matters if its execution semantics are sound and auditable. |
| [lambda-vm](https://blog.alignedlayer.com/aligned-monthly-recap-december-2025/) | Proving infrastructure / zkVMs | Minimal zkVM design | Minimal zkVM design is an auditability choice as much as a performance choice. |
| [Merces](https://eprint.iacr.org/2026/850) | Privacy-preserving systems | Private token transfer | Private token systems must design who computes and proves, not only what is hidden. |
| [Nondeterministic Autoprecompiles](https://powdr.org/blog/accelerating-ethereum-with-autoprecompiles) | Proving infrastructure / zkVMs | zkVM acceleration | zkVM performance work is moving from general execution toward automatic bottleneck specialization. |
| [Proof of Seed](https://www.soundness.xyz/blog/mpc-wallets-the-post-quantum-migration) | Security / correctness | Post-quantum migration | Cryptographic migration must preserve ownership continuity without exposing old secrets. |
| Origin Tags | Security / correctness | Fiat-Shamir bugs | ZK security often fails through missing bindings, not broken primitives. |
| [Making verifiable FHE practical](https://eprint.iacr.org/2025/286) | Primitives | FHE plus verifiability | Confidential computation is incomplete unless outsourced results are also verifiable. |
| [Witness Encryption from Arithmetic Affine Determinant Programs](https://eprint.iacr.org/2026/175) | Primitives | Witness encryption | Witness encryption ties access to proving a statement, not merely holding a key. |
| [eDAS](https://eprint.iacr.org/2026/325) | Privacy-preserving systems | Data availability and compliance | Public availability checks may still need privacy and regulatory boundaries. |
| Client-side Validation in Private Payment Protocols | Privacy-preserving systems | Private payment validation | Private payments can move validation to participants instead of global consensus. |
| [Jolt Atlas](https://arxiv.org/abs/2602.17452) | Broader verifiable applications | Verifiable ML inference | Verifiable inference is moving toward domain-specific proving, not just generic execution. |
| [Time-Space Trade-Offs for Sumcheck](https://eprint.iacr.org/2025/1473) | Proving infrastructure / zkVMs | Prover cost | Deployable provers are constrained by memory as much as asymptotic time. |
| A Security Guide to ZK Engineering | Security / correctness | Engineering failures | ZK systems fail when implementations encode a different guarantee than intended. |
| [ZK-AntiCheat](https://devfolio.co/projects/zkanticheat-74ee) | Privacy-preserving systems | Gaming anti-cheat | ZK can replace inspection with attestations of the properties a system actually needs. |
| Confidential and Verifiable Telemetry | Privacy-preserving systems | Operational data | Operational observability can be verifiable without exposing raw measurement data. |
| [Miden's ACE chiplet](https://0xmiden.github.io/air-script/) | Proving infrastructure / zkVMs | VM-specific acceleration | Recursive proving pushes VM design toward specialized internal components. |
| Efficient Non-Native Arithmetic from SNARKs for Integers | Proving infrastructure / zkVMs | Non-native arithmetic | Interoperability costs often hide in arithmetic the native field was not built for. |
| Resource-Sharing Permutations for Computational Integrity | Primitives | Computational integrity | Efficiency-oriented primitive design still needs explicit integrity accounting. |

Sources:

- [ZK Summit 14 event page](https://www.zksummit.com/)
- [ZK Summit 13 event page](https://zeroknowledge.fm/zksummit13/)
- [ZK Summit 12 event page](https://zeroknowledge.fm/zksummit12/)
- [ZK Summit 11 event page](https://zeroknowledge.fm/the-zero-knowledge-summit-11/)
- [ZK Summit 11-14 video playlist](https://www.youtube.com/watch?list=PLj80z0cJm8QFy2umHqu77a8dbZSqpSH54&v=nrfueiRDqL4)
