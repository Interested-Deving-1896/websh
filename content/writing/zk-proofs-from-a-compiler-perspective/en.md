---
title: "Zero-Knowledge Proofs, from a Compiler Perspective"
date: "2026-05-15"
tags: [zk, compilers, systems]
description: "How programs become relations, constraints, traces, and proofs."
language: en
---

# Zero-Knowledge Proofs, from a Compiler Perspective

> How programs become relations, constraints, traces, and proofs.

## 1. The Missing Middle

You wrote a program. The verifier checks a proof. What happened in between?

Most explanations of zero-knowledge proofs start from the cryptographic shape of the problem. There is a prover, a verifier, a statement, and a witness. The prover wants to convince the verifier that the statement is true, using the witness, without revealing the witness itself. This is the right starting point: it explains what makes zero-knowledge proofs useful and what kind of guarantee they are meant to provide.

But if you approach ZK as a developer, there is a missing middle. You usually do not begin with a mathematical relation written directly in the form consumed by a proof system. You write a program, a circuit, a guest application, a DSL, or some other description of computation. You say things like: this hash was computed correctly, this private value satisfies a range check, this state transition is valid, this program executed without violating its assertions.

Somehow, that source-level intent has to become the statement that the verifier will accept. That "somehow" is where the compiler perspective begins.

In practical ZK systems, the object being proven is usually not the source code itself. It is a relation derived from the source code: a circuit, a set of constraints, an AIR, a trace relation, an intermediate representation, or a VM execution semantics. A relation is the compiled object the proof is about. It defines what counts as a valid witness, a valid execution trace, or a valid set of private values.

This is why ZK systems are not only cryptographic systems. They are also compiler systems. Cryptography explains how a verifier can be convinced that some relation has been satisfied. The compiler stack decides which relation represents the program in the first place. Before the backend can prove anything, something has to translate developer intent into a proof-friendly form.

The rest of this article looks at ZK through that layer: not as an alternative to the cryptographic view, but as the part that connects cryptographic guarantees to programs people actually write.

## 2. The Proof Pipeline

At a high level, a ZK system has two paths that eventually meet inside the prover.

```text
source program / circuit DSL
  -> frontend compiler
  -> compiled relation
     circuit / constraints / AIR / trace relation / IR

private inputs + execution / witness generation
  -> witness / trace

compiled relation + witness / trace
  -> proving backend
  -> proof
  -> verifier
```

The first path is the compilation path. It starts from something a developer can write: a circuit DSL, a ZK language, guest code, or some other source representation. The frontend lowers that source into a proof-oriented representation: constraints, a circuit, an AIR, a trace relation, an IR, or VM semantics.

This path answers the question: what is being proven?

The second path is the witness or execution path. The prover has private inputs: the secret values, the private state, the credential, the program inputs, or the execution data that should not be fully revealed. From these values, the system produces a witness or an execution trace. In a circuit-oriented system, this may mean assigning values to wires and intermediate variables. In a zkVM, it may mean executing the guest program and recording a trace of VM steps, memory events, and other proof-relevant execution data.

This path answers a different question: what concrete values are claimed to satisfy the relation?

The proving backend is where these paths meet. It receives, directly or through additional lowering, a compiled relation and a satisfying witness or trace. Then it applies the proof protocol and produces a proof that can be checked by a verifier without re-running the private computation or seeing the private data.

This gives a useful frontend/backend split. The frontend or compiler layer decides what relation represents the program. The witness generation or execution layer produces the concrete private data that should satisfy that relation. The backend decides how to prove satisfaction of that relation efficiently and soundly.

The boundary is not always perfectly clean. Some frontends are designed around a particular backend. Some backends impose strong constraints on the shape of the relations they can prove efficiently. Some systems expose an intermediate layer; others couple the language, arithmetization, and backend more tightly.

Still, the split is useful because it prevents a common confusion. A proof is not made directly from source code. A proof is made from a relation and a witness or trace. The source code matters because it is supposed to compile into the right relation. The witness matters because it is supposed to satisfy that relation. The backend matters because it is supposed to convince the verifier that this satisfaction happened without revealing more than necessary.

Once this lifecycle is visible, many ZK design questions start to look like compiler questions. What should the source language guarantee? What should the IR make explicit? Which values are public inputs, which are private witnesses, and which are intermediate artifacts? These are not peripheral details. They define the object that the proof is actually about.

## 3. A Verifier Does Not Check Your Source Code

The compiler perspective becomes concrete when you look at what the verifier actually checks.

Suppose the source program contains a claim like this:

```rust
assert!(age >= 18);
```

At the source level, the intended statement is easy to say: there is a private age, and that age is at least 18. In a real credential system, the claim would also need to bind the issuer, subject, credential contents, and public output. But even the tiny version is enough to show the compiler issue.

The verifier does not look at this line of source code and decide whether it is true. The verifier checks a proof against a compiled relation, public inputs, and some verification key or program identity. In simplified form, the flow is:

```text
source claim:    this private age is at least 18
compiled claim:  relation(public_inputs, witness)
verifier checks: proof of the compiled claim
```

For the proof to mean what the developer thinks it means, the source-level claim has to survive lowering. The frontend must decide how `age` is represented, what field or integer semantics apply, whether the comparison is range-checked, how the boolean result is constrained, and how the public output is bound to the private witness. If this is a zkVM, the system must also bind the proof to the right program or image and to the semantics of the VM execution being proven.

This is different from normal software in a subtle but important way. When you run a program on a CPU, the machine executes instructions according to a hardware and language model. If the program computes `age >= 18`, the result follows from the runtime semantics of that program. In ZK, the verifier is observing a proof that some compiled relation has been satisfied.

That means the relation is the verifier's world. If the relation says "there exists a private value," then a proof of that relation only proves existence. If the relation says "there exists a private age in the right range, bound to this credential, whose comparison with 18 produces this public eligibility result," then the proof can support the stronger claim. The difference is not in the cryptographic backend alone. It is in what the compiler, circuit, or VM semantics made explicit before the proof was generated.

This is the central shift. From the outside, a verifier receives a compact proof. From the compiler perspective, that proof is the end of a translation process. The verifier's trust attaches to the compiled relation, not to an informal memory of what the source code was supposed to mean.

## 4. Source Operations Become Compiled Obligations

Return to the small line of code:

```rust
assert!(age >= 18);
```

In ordinary software, this line inherits a large amount of meaning from the language and the machine. `age` has a type. That type has a range. The comparison operator has defined integer semantics. The assertion has control-flow behavior. If `age` is a `u32`, overflow, representation, and comparison are already part of the language and target model.

In a proof target, those facts have to appear in a form the proof system can check. Most proving systems do not directly understand "a 32-bit unsigned integer comparison" as a primitive source-level concept. They understand field elements, constraints, gates, lookups, trace columns, memory events, or VM transition rules. A field element is not automatically a `u32`; a comparison is not automatically a CPU comparison; a boolean-looking value is not automatically constrained to be either `0` or `1`.

So an operation that looks primitive in source code becomes a bundle of compiled obligations. For `age >= 18`, the compiler, circuit, or VM semantics may need to account for several things:

```text
age is represented in the expected domain
age is in the intended range
the comparison uses the intended integer semantics
the comparison result is boolean
the asserted result is constrained to be true
the public output is bound to that result
```

If this is a credential proof, the obligations grow. The relation may also need to bind the credential to an issuer, bind the private age or birthdate to that credential, bind the proof to the subject or holder, and expose only the public eligibility result. The comparison itself is only one part of the claim.

Different systems move this burden to different places. In a circuit-first system, the developer may directly wire the range check, comparison, and boolean constraint. In a higher-level ZK language, the source code may look more ordinary while the compiler and backend emit the necessary constraints. In a zkVM, the program may execute an instruction sequence, but the proven VM semantics still have to account for the relevant instructions, registers, memory, public outputs, and program identity.

The user experience changes. The obligation does not disappear.

## 5. Execution Is Not Enforcement

The next trap is that computing a value and enforcing a value are different things.

In normal software, we often blur those ideas. If a program computes a value and then asserts it, the runtime either continues or fails. In a proof system, a witness generator may compute many values while preparing the proof, but only the compiled checks are enforced.

That distinction is easy to miss, so make it concrete:

```text
computed:    is_adult = age >= 18
forgotten:   constrain is_adult == true
result:      the witness may contain is_adult,
             but the proof may not enforce the intended claim
```

Circuit languages often expose the distinction directly. In Circom-style development, assigning a signal during witness generation and adding the constraint that makes the value checkable are different concepts. Higher-level languages may hide more of the wiring, but they still need the same separation underneath. Noir, for example, has a notion of unconstrained computation that can be useful for witness generation, but such computation is not by itself the same thing as a constraint the proof enforces.

One way to summarize the split is:

```text
assignment      computes a witness value
constraint      makes a relation checkable
private input   supplies hidden data
public input    defines the visible claim
```

The important word is not "compute." It is "enforce." ZK compilers, circuit builders, and VM arithmetizations need to produce artifacts that prevent the wrong witness or trace from satisfying the relation.

This is also the first place where compiler correctness starts to become a verifier-trust issue. If the compiled relation forgets an obligation, the proof system may still work perfectly; it may just prove a weaker or different statement.

## 6. The Target Is Not a CPU

These obligations exist because the compiler target is not a CPU instruction stream. It is a proof-oriented representation of computation.

Ordinary compilers lower source programs into representations that a CPU, VM, or runtime can execute. Their main target is an execution environment: instructions, registers, memory, ABI conventions, object files, bytecode, or runtime calls. A ZK compiler has a stranger target. It must produce, or target, a representation whose correctness can be checked by a proof system.

There are many names for those representations:

```text
arithmetic circuits
R1CS-style constraints
PLONKish gates and lookups
AIR and trace relations
constraint IRs such as ACIR
MLIR-style ZK IRs such as LLZK
zkVM execution semantics
```

The useful umbrella term is arithmetization: the part of the compiler stack that turns computation into algebraic checks. It decides how a program, circuit, machine step, memory access, comparison, hash operation, or state transition becomes something a proof system can verify.

But arithmetization does not always happen in the same place.

In circuit DSLs and many ZK languages, the toolchain often produces a relation specific to the program. The compiled artifact is close to the computation the developer wrote: constraints, gates, witness assignments, lookups, or an IR that can later be lowered into a proving backend.

In zkVMs, the boundary moves. The VM is usually arithmetized once, and ordinary programs compile to the VM's instruction set or bytecode. Proving a program then means proving that a particular execution trace is accepted by the VM transition relation. The application code can feel much more like normal software, as in RISC Zero or SP1, but the proof is still about valid execution of a specific program under specific VM semantics.

Other systems sit in between. Cairo programs are compiled toward a STARK-friendly execution model. Halo2 exposes a lower-level circuit API where developers work close to columns, gates, selectors, and lookups. Noir exposes a higher-level language and lowers through ACIR so that proving backends can consume the compiled relation. LLZK makes another compiler boundary visible by providing an IR layer for ZK circuit representations and analysis.

The important point is not that one of these targets is the real ZK target and the others are abstractions. The point is that every practical system has to draw a boundary somewhere between source-level intent and proof-level checks. Some systems expose constraints. Some expose a language. Some expose a VM. Some expose an IR. Each choice changes where developers think about semantics, performance, debugging, and trust.

A normal compiler asks, among other things, "Can this program run correctly on the target machine?" A ZK compiler has to ask a slightly different question: "Can the verifier be convinced that this relation was satisfied, or that this machine execution was valid, without seeing everything the prover knows?"

## 7. Backends, Boundaries, and Cost Models

The backend is where the proof protocol becomes concrete. It takes a relation, or some backend-specific lowering of that relation, and turns satisfaction of that relation into a proof. Depending on the system, this may involve commitments, transcripts, setup assumptions, recursion, aggregation, and verifier machinery.

From the compiler perspective, the important point is simpler: the backend decides how a relation is proven, but it does not get to ignore the shape of the relation it receives.

This is why frontend and backend boundaries matter. A frontend may emit an IR that can be consumed by different backends. Noir source, for example, lowers toward ACIR before backend-specific proving work. Other systems are more tightly coupled: the source language, arithmetization, and proving system are designed together. A clean boundary gives portability and analysis opportunities; tighter coupling can expose better performance or a simpler mental model.

But separable does not mean independent. The backend shapes what is cheap, natural, or even available to the frontend. A lookup-friendly backend changes how range checks or table-heavy operations are encoded. A STARK-oriented trace system changes how one thinks about execution length and transition constraints. A recursive proof system changes which intermediate proofs are worth producing.

That is where the ZK cost model diverges from the ordinary software cost model. Fast to run is not necessarily cheap to prove.

In normal software, we often optimize around CPU cycles, memory locality, allocation patterns, instruction count, or wall-clock latency. Those costs still matter in a ZK prover, but they are not the whole story. The proof-oriented costs may be constraint or gate count, trace length, lookup usage, witness size, prover time, proof size, verifier time, setup cost, or recursion overhead.

This is why ordinary-looking operations can have surprising proof costs. Hashing, elliptic-curve operations, bit manipulation, comparisons, and random memory access may be cheap or routine in normal software, but expensive in a circuit or trace. Many systems therefore specialize these operations through builtins, precompiles, lookup tables, chiplets, or VM-specific acceleration instead of proving them in the most direct way.

There are really several cost models stacked together:

```text
source-level cost:        is the program pleasant and efficient to write?
frontend cost:            what relation, constraints, or trace does it produce?
backend cost:             how expensive is it to prove and verify that relation?
deployment cost:          setup, recursion, aggregation, on-chain verification
```

These costs are related, but they are not the same thing. A source-level rewrite can reduce gates while increasing witness complexity. A VM abstraction can make development easier while producing a longer execution trace. A backend choice can make verification tiny while requiring a setup ceremony or a heavier prover.

The backend also gives us a useful limit on responsibility. A sound backend can prove that a relation was satisfied. It cannot make an incorrect relation mean the right thing. If the frontend forgot a range check, erased a necessary constraint, or failed to bind the right public claim, backend soundness does not repair that mistake. That issue belongs to the compiler side of the trust story, which we will return to shortly.

## 8. Where Each Product Draws the Line

With that model in place, existing tools become easier to place. The point is not to rank them. It is to ask where each system draws the line between source intent, compiled relation, execution trace, and proving backend.

| Tool family | Developer writes | Relation / execution representation | Proving layer |
|---|---|---|---|
| Circom | circuit DSL | R1CS-style constraints | snarkjs / Groth16 / PLONK workflows |
| Noir | ZK language | ACIR | Barretenberg / other backends |
| Cairo | Cairo program | CASM plus CairoVM trace / AIR | STARK prover |
| RISC Zero / SP1 | Rust or RISC-V guest | RISC-V program execution trace | zkVM prover |
| Halo2 | circuit API | PLONKish circuit | Halo2 proving system |
| LLZK | outputs from ZK DSLs or circuit systems | MLIR-based ZK circuit IR | analysis, verification, and lowering toward proving targets |

This table is simplified, and not every row is the same kind of object. LLZK is closer to an intermediate compiler layer than a developer-facing proving framework; it is useful here precisely because it makes the middle visible.

Circom asks the developer to think close to constraints. That gives control, but also exposes the risk of underconstraining. Halo2 also lives close to the proof target, though through a different PLONKish programming model.

Noir raises the source level. The developer writes something closer to a normal program, while ACIR acts as a boundary between the language and proving backends. That boundary gives the ecosystem a place to analyze, optimize, and retarget relations.

Cairo, RISC Zero, and SP1 lean more toward execution traces. The developer writes a program, the system executes a machine, and the proof is about valid execution of that machine. This can make ZK feel more like ordinary programming, but it does not remove the proof-oriented target. It moves the target into VM semantics, trace constraints, program identity, public outputs, and prover cost.

The useful question is therefore not "which tool has a compiler?" They all have compiler machinery somewhere. The useful question is: where does the tool ask the developer to think: constraints, IR, backend gates and lookups, VM cycles, public outputs, program identity, or verifier cost?

Once the map is visible, the landscape becomes less mysterious. The systems look different because they choose different boundaries, but they all still have to solve the same underlying problem: turn source-level intent into a relation, produce a witness or trace, and prove that the relation was satisfied.

## 9. Compiler Correctness Is Verifier Trust

In ZK, compiler correctness is not just an implementation detail. It is part of what the verifier ends up trusting.

A proving backend can be sound and still prove the wrong thing if the relation it receives is wrong. "The proof is valid" and "the developer's intended claim was proven" are not the same statement. A proof is valid relative to a relation, public inputs, verifier key, program identity, and protocol. If that relation is weaker than intended, the proof may still be perfectly valid.

The small age example gives the shape of the problem:

```text
intended claim: this private age is at least 18
compiled claim: there exists some private age
proof system:   sound
verifier result: confidence in the wrong statement
```

This is not a cryptographic failure in the narrow sense. The backend may have done exactly what it promised: prove that the compiled relation is satisfiable. The failure is earlier: the source-level claim did not survive compilation into the relation the verifier checked.

The failure can happen at several layers. A developer may forget an obligation in the source-level claim. A frontend compiler may lower the source into the wrong relation. An IR pass or optimizer may remove a constraint that carried semantic meaning. A backend or verifier implementation may have its own soundness bug. An integration layer may bind the proof to the wrong public input, key, program image, journal, or receipt.

In ZK, a bug at any layer can become a verifier-trust bug. The result may not be a proof that fails, but a valid proof that convinces the verifier of the wrong claim. The important question is not only "can the backend prove this relation?" It is "does this relation faithfully represent the claim we meant to prove?"

Formal verification is becoming more relevant for the same reason. On the frontend side, IRs and analysis frameworks can help reason about circuits, constraints, and VM semantics: are checks present, transformations meaning-preserving, and public inputs bound correctly? [LLZK](https://github.com/project-llzk/llzk-lib) is one example at the IR layer, while zkVM examples such as Lean models of [RISC Zero](https://github.com/risc0/risc0-lean4) and [Lean proofs for SP1 chip constraints](https://github.com/NethermindEth/sp1-fv-poc) show the same pressure at the VM/circuit layer. On the backend side, [ArkLib](https://github.com/Verified-zkEVM/ArkLib) points in a related but different direction: formalizing SNARK protocol components and their completeness or soundness arguments in Lean.

These efforts do not remove the need for cryptography. They clarify which relation the cryptographic guarantee attaches to. A proof can be cryptographically valid while the claim it proves is not the claim the developer intended. Compiler correctness is what makes verifier trust attach to the right claim.

## 10. Cryptography Guarantees; Compilation Defines

Zero-knowledge proofs are cryptographic objects. The proof protocol is what gives the verifier confidence that a statement was proven without revealing the private witness.

But practical ZK systems are also compiler systems. Developers write programs, circuits, guest code, and DSLs. Toolchains turn those artifacts into relations, constraints, traces, IRs, and verification artifacts. Provers combine those compiled objects with witnesses or execution traces. Verifiers check the final proof against public inputs, keys, program identities, or receipts.

Seen this way, practical ZK is not only about choosing a proof system. It is also about the path from program to relation: how source operations become constraints, how execution becomes a witness or trace, where the frontend/backend boundary sits, and which costs or trust assumptions each layer introduces.

Cryptography gives the verifier confidence. The compiler stack decides what that confidence attaches to.
