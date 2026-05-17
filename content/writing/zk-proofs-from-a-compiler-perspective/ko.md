---
title: "컴파일러 관점에서 보는 영지식 증명"
date: "2026-05-15"
tags: [explainer, zk, compilers, proof-systems, arithmetization]
description: "프로그램이 relation, constraint, trace, proof가 되는 과정을 컴파일러 관점에서 설명합니다."
language: ko
---

# 컴파일러 관점에서 보는 영지식 증명

> How programs become relations, constraints, traces, and proofs.

## 1. The Missing Middle

개발자는 프로그램을 작성합니다. verifier는 proof를 검증합니다. 그 사이에서는 무슨 일이 일어날까요?

영지식 증명을 설명할 때는 보통 암호학적인 구조에서 출발합니다. prover가 있고, verifier가 있고, statement가 있고, witness가 있습니다. prover는 witness를 직접 공개하지 않으면서도, 어떤 statement가 참이라는 것을 verifier에게 납득시켜야 합니다. 이 설명은 맞습니다. 영지식 증명이 어떤 보장을 주는지, 왜 유용한지 이해하려면 반드시 필요한 관점입니다.

하지만 개발자의 입장에서 ZK를 바라보면, 이 설명만으로는 중간이 비어 있습니다. 개발자는 보통 proof system이 바로 소비할 수 있는 수학적 relation을 직접 쓰는 것에서 시작하지 않습니다. 개발자가 쓰는 것은 프로그램, circuit, guest application, DSL, 혹은 어떤 계산을 설명하는 소스 코드에 가깝습니다. 이를테면 이런 말을 하고 싶은 것입니다. 이 hash는 올바르게 계산되었다. 이 private value는 특정 범위 안에 있다. 이 state transition은 유효하다. 이 프로그램은 assertion을 위반하지 않고 실행되었다.

그렇다면 이 source-level intent는 어떻게 verifier가 받아들이는 statement가 될까요? 바로 이 지점에서 컴파일러 관점이 시작됩니다.

실제 ZK 시스템에서 증명되는 대상은 대개 source code 자체가 아닙니다. 증명되는 대상은 source code로부터 만들어진 relation입니다. circuit일 수도 있고, constraint set일 수도 있고, AIR일 수도 있고, trace relation일 수도 있고, intermediate representation일 수도 있고, VM execution semantics일 수도 있습니다. relation은 proof가 다루는 컴파일된 대상입니다. 무엇이 valid witness인지, 무엇이 valid execution trace인지, 어떤 private value들이 statement를 만족한다고 볼 수 있는지를 정의합니다.

그래서 ZK 시스템은 암호학 시스템이면서 동시에 컴파일러 시스템입니다. 암호학은 어떤 relation이 만족되었다는 사실을 verifier가 어떻게 믿을 수 있는지 설명합니다. 반면 compiler stack은 어떤 relation이 프로그램을 대표하는지 결정합니다. backend가 무엇이든 증명하려면, 그 전에 누군가는 developer intent를 proof-friendly한 형태로 바꿔야 합니다.

이 글은 바로 그 layer를 통해 ZK를 바라봅니다. 암호학적 관점을 대체하려는 것이 아닙니다. 오히려 암호학적 보장이 실제 개발자가 작성한 프로그램과 어떻게 연결되는지를 보기 위한 관점입니다.

## 2. The Proof Pipeline

크게 보면 ZK 시스템에는 두 개의 흐름이 있고, 이 둘은 prover 안에서 만납니다.

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

첫 번째는 compilation path입니다. 출발점은 개발자가 작성할 수 있는 어떤 형태입니다. circuit DSL, ZK language, guest code, 혹은 다른 source representation일 수 있습니다. frontend는 이 source를 proof-oriented representation으로 낮춥니다. 시스템에 따라 그 결과는 constraint, circuit, AIR, trace relation, IR, 혹은 VM semantics처럼 보일 수 있습니다.

이 흐름이 답하는 질문은 이것입니다. 무엇을 증명할 것인가?

두 번째는 witness 또는 execution path입니다. prover에게는 private input이 있습니다. secret value, private state, credential, program input, 혹은 전부 공개하고 싶지는 않은 execution data입니다. 시스템은 이 값들로부터 witness나 execution trace를 만듭니다. circuit 중심 시스템에서는 wire와 intermediate variable에 값을 할당하는 과정일 수 있습니다. zkVM에서는 guest program을 실행하고, VM step, memory event, 기타 proof에 필요한 execution data를 trace로 기록하는 과정일 수 있습니다.

이 흐름이 답하는 질문은 조금 다릅니다. 어떤 구체적인 값들이 이 relation을 만족한다고 주장하는가?

proving backend는 이 두 흐름이 만나는 곳입니다. backend는 compiled relation과 그 relation을 만족하는 witness 또는 trace를 받습니다. 직접 받기도 하고, 추가적인 lowering을 거친 형태로 받기도 합니다. 그 다음 proof protocol을 적용해 proof를 만듭니다. 그 결과 verifier는 private computation을 다시 실행하지 않고, private data를 직접 보지 않고도 proof를 확인할 수 있습니다.

이렇게 보면 frontend와 backend의 구분이 선명해집니다. frontend 또는 compiler layer는 어떤 relation이 프로그램을 대표하는지 결정합니다. witness generation 또는 execution layer는 그 relation을 만족해야 하는 구체적인 private data를 만듭니다. backend는 그 relation이 만족되었다는 사실을 어떻게 효율적이고 sound하게 증명할지를 담당합니다.

물론 이 경계가 항상 깔끔하게 나뉘는 것은 아닙니다. 어떤 frontend는 특정 backend를 염두에 두고 설계됩니다. 어떤 backend는 자신이 효율적으로 증명할 수 있는 relation의 형태에 강한 제약을 둡니다. 어떤 시스템은 중간 IR layer를 노출하고, 어떤 시스템은 language, arithmetization, backend를 더 강하게 결합합니다.

그래도 이 구분은 중요합니다. 흔한 오해 하나를 피하게 해주기 때문입니다. proof는 source code에서 직접 만들어지는 것이 아닙니다. proof는 relation과 witness 또는 trace로부터 만들어집니다. source code가 중요한 이유는 그것이 올바른 relation으로 compile되어야 하기 때문입니다. witness가 중요한 이유는 그것이 relation을 만족해야 하기 때문입니다. backend가 중요한 이유는 이 만족 관계를 필요한 것 이상 공개하지 않으면서 verifier에게 납득시켜야 하기 때문입니다.

이 lifecycle이 보이기 시작하면, 많은 ZK 설계 문제가 컴파일러 문제처럼 보이기 시작합니다. source language는 무엇을 보장해야 할까요? IR은 무엇을 명시적으로 드러내야 할까요? 어떤 값은 public input이고, 어떤 값은 private witness이고, 어떤 값은 단지 중간 산출물일까요? 이것들은 주변부 디테일이 아닙니다. proof가 실제로 무엇에 대한 것인지를 결정하는 질문들입니다.

## 3. A Verifier Does Not Check Your Source Code

컴파일러 관점은 verifier가 실제로 무엇을 확인하는지 보면 더 구체적으로 드러납니다.

예를 들어 source program 안에 이런 claim이 있다고 해봅시다.

```rust
assert!(age >= 18);
```

source level에서 의도는 단순합니다. 어떤 private age가 있고, 그 age가 18 이상임을 보이고 싶습니다. 실제 credential system이라면 비교 하나만으로는 부족합니다. issuer, subject, credential contents, public output도 함께 묶여야 합니다. 하지만 아주 작은 예시만으로도 compiler issue는 충분히 드러납니다.

verifier는 이 source code 한 줄을 보고 참인지 거짓인지 판단하지 않습니다. verifier가 확인하는 것은 compiled relation, public input, 그리고 verification key나 program identity에 대한 proof입니다. 단순화하면 흐름은 이렇습니다.

```text
source claim:    this private age is at least 18
compiled claim:  relation(public_inputs, witness)
verifier checks: proof of the compiled claim
```

proof가 개발자가 의도한 의미를 가지려면, source-level claim이 lowering 과정에서 살아남아야 합니다. frontend는 `age`를 어떻게 표현할지 결정해야 합니다. 어떤 field 또는 integer semantics를 적용할지, comparison에 range check가 필요한지, boolean 결과가 제대로 constrain되는지, public output이 private witness와 어떻게 binding되는지를 결정해야 합니다. zkVM이라면 proof가 올바른 program image에 묶여 있는지, 그리고 어떤 VM execution semantics를 증명하고 있는지도 중요합니다.

이 점은 일반적인 소프트웨어 실행과 미묘하지만 중요하게 다릅니다. CPU 위에서 프로그램을 실행할 때, machine은 hardware와 language model이 정한 semantics에 따라 instruction을 실행합니다. 프로그램이 `age >= 18`을 계산했다면, 그 결과는 해당 runtime semantics로부터 나옵니다. 하지만 ZK에서 verifier가 보는 것은 그 runtime이 아니라, 어떤 compiled relation이 만족되었다는 proof입니다.

따라서 relation이 verifier의 세계입니다. relation이 단지 "어떤 private value가 존재한다"고 말한다면, 그 proof는 존재성만 증명합니다. relation이 "올바른 범위의 private age가 있고, 그것이 이 credential에 binding되어 있으며, 18과 비교한 결과가 이 public eligibility output을 만든다"고 말한다면, proof는 더 강한 claim을 뒷받침할 수 있습니다. 차이는 cryptographic backend에만 있지 않습니다. proof가 만들어지기 전에 compiler, circuit, VM semantics가 무엇을 명시적으로 표현했는지에 있습니다.

핵심 전환은 이것입니다. 바깥에서 보면 verifier는 작은 proof 하나를 받습니다. 하지만 compiler perspective에서 보면 그 proof는 translation process의 마지막 결과입니다. verifier의 신뢰는 source code가 의도했다는 막연한 기억에 붙는 것이 아니라, 실제로 컴파일된 relation에 붙습니다.

## 4. Source Operations Become Compiled Obligations

앞에서 본 코드를 다시 보겠습니다.

```rust
assert!(age >= 18);
```

일반적인 소프트웨어에서 이 한 줄은 언어와 machine으로부터 많은 의미를 물려받습니다. `age`에는 type이 있고, 그 type에는 range가 있습니다. comparison operator에는 정의된 integer semantics가 있고, assertion에는 control-flow behavior가 있습니다. `age`가 `u32`라면 overflow, representation, comparison은 이미 언어와 target model의 일부입니다.

하지만 proof target에서는 이런 사실들이 자동으로 주어지지 않습니다. proof system이 다루는 것은 field element, algebraic constraint, gate, lookup, trace column, memory event, 혹은 VM transition rule에 가깝습니다. field element는 자동으로 `u32`가 아니고, comparison은 자동으로 CPU comparison이 아니며, boolean처럼 보이는 값도 자동으로 `0` 또는 `1`로 constrain되어 있지 않습니다.

그래서 source code에서는 primitive처럼 보이는 operation이 proof target에서는 여러 개의 compiled obligation으로 바뀝니다.

```text
age is represented in the expected domain
age is in the intended range
the comparison uses the intended integer semantics
the comparison result is boolean
the asserted result is constrained to be true
the public output is bound to that result
```

credential proof라면 obligation은 더 늘어납니다. credential이 올바른 issuer에 의해 발급되었는지, private age나 birthdate가 그 credential에 묶여 있는지, proof가 올바른 subject나 holder에 묶여 있는지, public eligibility result만 노출되는지도 표현해야 할 수 있습니다. comparison은 claim의 한 조각일 뿐입니다.

시스템마다 이 부담을 다른 위치에 둡니다. circuit-first system에서는 개발자가 range check, comparison, boolean constraint를 직접 연결할 수 있습니다. 더 높은 수준의 ZK language에서는 compiler와 backend가 필요한 constraint를 만들어줄 수 있습니다. zkVM에서는 프로그램이 실제 instruction sequence로 실행될 수 있지만, VM의 proven execution semantics가 instruction, register, memory, public output, program identity를 올바르게 묶어줘야 합니다.

개발자 경험은 달라집니다. 하지만 obligation이 사라지는 것은 아닙니다.

## 5. Execution Is Not Enforcement

다음으로 조심해야 할 점은 값을 계산하는 것과 값을 enforce하는 것이 다르다는 것입니다.

일반적인 소프트웨어에서는 이 둘을 자주 섞어 생각합니다. 프로그램이 값을 계산하고 곧바로 assertion으로 확인한다면 runtime은 계속 진행되거나 실패합니다. 하지만 proof system에서는 witness generator가 많은 값을 계산할 수 있어도, proof가 enforce하는 것은 compiled check뿐입니다.

차이를 구체적으로 쓰면 이렇습니다.

```text
computed:    is_adult = age >= 18
forgotten:   constrain is_adult == true
result:      the witness may contain is_adult,
             but the proof may not enforce the intended claim
```

circuit language는 이 차이를 비교적 직접적으로 드러냅니다. Circom-style development에서는 witness generation 중 signal에 값을 할당하는 것과, 그 값이 검증 가능하도록 constraint를 추가하는 것이 다른 개념입니다. higher-level language는 이 wiring을 더 많이 숨길 수 있지만, 내부적으로는 같은 분리가 필요합니다. Noir의 unconstrained computation도 witness generation에 유용할 수 있지만, 그 자체가 proof가 enforce하는 constraint는 아닙니다.

간단히 요약하면 다음과 같습니다.

```text
assignment      computes a witness value
constraint      makes a relation checkable
private input   supplies hidden data
public input    defines the visible claim
```

중요한 단어는 "compute"가 아니라 "enforce"입니다. ZK compiler, circuit builder, VM arithmetization은 올바른 답을 계산할 수 있는 artifact만 만들면 안 됩니다. 잘못된 답이 relation을 만족하지 못하도록 만들어야 합니다.

이 지점에서 compiler correctness는 verifier trust의 문제가 되기 시작합니다. compiled relation이 필요한 obligation을 빠뜨리면, proof system은 여전히 완벽하게 동작할 수 있습니다. 다만 더 약하거나 다른 statement를 증명하게 됩니다.

## 6. The Target Is Not a CPU

이런 obligation이 생기는 이유는 compiler target이 CPU instruction stream이 아니기 때문입니다. ZK의 target은 computation을 proof-friendly하게 표현한 것입니다.

물론 어떤 ZK 시스템은 실제로 프로그램을 CPU-like instruction set, bytecode, 혹은 virtual machine으로 compile합니다. zkVM은 Rust로 guest program을 작성하게 해주고, ordinary execution에 가까운 개발 경험을 줄 수 있습니다. 하지만 최종 proof target은 일반적인 의미의 "CPU가 이 프로그램을 실행했다"가 아닙니다. 특정 machine execution이 증명 대상인 VM semantics 아래에서 valid하다는 relation입니다.

일반적인 compiler는 source program을 CPU, VM, runtime이 실행할 수 있는 representation으로 낮춥니다. 반면 ZK compiler는 proof system이 correctness를 확인할 수 있는 representation을 만들거나 겨냥해야 합니다.

이런 representation에는 여러 이름이 있습니다.

```text
arithmetic circuits
R1CS-style constraints
PLONKish gates and lookups
AIR and trace relations
constraint IRs such as ACIR
MLIR-style ZK IRs such as LLZK
zkVM execution semantics
```

이것들을 묶어서 볼 때 유용한 단어가 arithmetization입니다. arithmetization은 computation을 algebraic check로 바꾸는 compiler stack의 한 부분입니다.

하지만 arithmetization은 항상 같은 위치에서 일어나지 않습니다. circuit DSL이나 많은 ZK language에서는 toolchain이 program-specific relation을 만드는 경우가 많습니다. compiled artifact는 constraint, gate, witness assignment, lookup, 혹은 이후 proving backend로 낮출 수 있는 IR에 가깝습니다.

zkVM에서는 boundary가 다르게 이동합니다. VM은 대개 한 번 arithmetize되고, ordinary program은 VM의 instruction set이나 bytecode로 compile됩니다. 그러면 어떤 프로그램을 증명한다는 것은 특정 execution trace가 VM transition relation에 의해 accepted된다는 것을 증명하는 일이 됩니다. 그래서 RISC Zero나 SP1 같은 시스템은 Rust에 가까운 개발 경험을 제공하면서도, 내부적으로는 compiler/proof system입니다.

다른 시스템들은 그 사이 어딘가에 있습니다. Cairo program은 STARK-friendly execution model 쪽으로 compile됩니다. Halo2는 column, gate, selector, lookup에 가까운 lower-level circuit API를 노출합니다. Noir는 higher-level language를 제공하고 ACIR를 거쳐 proving backend가 소비할 수 있는 relation으로 낮춥니다. LLZK는 ZK circuit representation과 analysis를 위한 IR layer를 제공함으로써 또 다른 compiler boundary를 명시적으로 드러냅니다.

중요한 점은 이 중 하나만이 진짜 ZK target이고 나머지는 abstraction이라는 것이 아닙니다. 모든 practical system은 source-level intent와 proof-level check 사이의 어딘가에 boundary를 그어야 합니다. 어떤 시스템은 constraint를 노출합니다. 어떤 시스템은 language를 노출합니다. 어떤 시스템은 VM을 노출합니다. 어떤 시스템은 IR을 노출합니다. 이 선택은 개발자가 semantics, performance, debugging, trust를 어디에서 생각해야 하는지를 바꿉니다.

일반적인 compiler가 "이 프로그램이 target machine에서 올바르게 실행될 수 있는가?"를 묻는다면, ZK compiler는 조금 다른 질문을 함께 물어야 합니다. "prover가 아는 모든 것을 보여주지 않고도, 이 relation이 만족되었거나 이 machine execution이 valid했다는 것을 verifier에게 납득시킬 수 있는가?"

## 7. Backends, Boundaries, and Cost Models

backend는 proof protocol이 구체적인 형태를 갖는 곳입니다. backend는 relation, 혹은 backend-specific하게 lowering된 relation을 받아서, 그 relation이 만족되었다는 사실을 proof로 만듭니다. 구체적인 시스템에 따라 commitment, transcript, setup assumption, recursion, aggregation, verifier machinery 같은 것들이 여기에 들어갑니다.

하지만 compiler 관점에서 더 중요한 점은 단순합니다. backend는 relation을 어떻게 증명할지 결정합니다. 하지만 자신이 받은 relation의 shape을 무시할 수는 없습니다.

그래서 frontend/backend boundary가 중요합니다. 어떤 frontend는 여러 backend가 소비할 수 있는 IR을 냅니다. 예를 들어 Noir source는 backend-specific proving work로 가기 전에 ACIR 쪽으로 lowering됩니다. 반대로 source language, arithmetization, proving system이 더 강하게 결합된 시스템도 있습니다. 깔끔한 boundary는 portability와 analysis 기회를 줍니다. 더 타이트한 boundary는 성능이나 단순한 mental model을 줄 수 있습니다.

하지만 separable하다는 것이 independent하다는 뜻은 아닙니다. backend는 frontend에게 무엇이 싸고, 자연스럽고, 가능한지를 형성합니다. lookup-friendly backend에서는 range check나 table-heavy operation을 다르게 encoding하게 됩니다. STARK-oriented trace system에서는 execution length와 transition constraint가 중요한 감각이 됩니다. recursive proof system에서는 어떤 intermediate proof를 만들 가치가 있는지도 달라집니다.

여기서 ZK의 cost model은 일반적인 software cost model과 갈라집니다. **fast to run is not necessarily cheap to prove.**

일반적인 소프트웨어에서는 CPU cycle, memory locality, allocation pattern, instruction count, wall-clock latency 같은 것을 중심으로 최적화합니다. ZK prover에서도 이런 비용이 완전히 사라지는 것은 아니지만, 전부는 아닙니다. proof-oriented cost에는 constraint count, gate count, trace length, lookup usage, witness size, prover time, proof size, verifier time, setup cost, recursion overhead 같은 것들이 있습니다.

그래서 평범해 보이는 operation이 proof 안에서는 의외로 비쌀 수 있습니다. hash, elliptic-curve operation, bit manipulation, comparison, random memory access는 일반 소프트웨어에서는 routine한 작업일 수 있지만, circuit이나 trace에서는 비쌀 수 있습니다. 그래서 많은 시스템은 이런 연산을 그대로 증명하기보다 builtin, precompile, lookup table, chiplet, VM-specific acceleration 같은 별도 경로로 특수화합니다.

cost model은 한 층이 아닙니다.

```text
source-level cost:        is the program pleasant and efficient to write?
frontend cost:            what relation, constraints, or trace does it produce?
backend cost:             how expensive is it to prove and verify that relation?
deployment cost:          setup, recursion, aggregation, on-chain verification
```

이 비용들은 서로 관련되어 있지만 같지는 않습니다. source-level rewrite가 gate 수를 줄이면서 witness complexity를 늘릴 수 있습니다. VM abstraction은 개발을 쉽게 만들지만 더 긴 execution trace를 만들 수 있습니다. backend 선택은 verifier를 작게 만들면서 setup ceremony나 더 무거운 prover를 요구할 수 있습니다.

backend의 책임에도 한계가 있습니다. sound한 backend는 relation이 만족되었다는 것을 증명할 수 있습니다. 하지만 잘못된 relation을 올바른 의미로 바꿔주지는 못합니다. frontend가 range check를 빠뜨렸거나, 필요한 constraint를 지웠거나, public claim을 잘못 binding했다면 backend soundness가 그 실수를 고쳐주지는 않습니다. 이 문제는 compiler side의 trust 이야기로 이어집니다.

## 8. Where Each Product Draws the Line

이 모델을 가지고 보면 기존 도구들의 위치가 더 잘 보입니다. 여기서 중요한 것은 순위를 매기는 것이 아닙니다. 각 시스템이 source intent, compiled relation, execution trace, proving backend 사이에서 boundary를 어디에 긋는지 보는 것입니다.

| Tool family | Developer writes | Relation / execution representation | Proving layer |
|---|---|---|---|
| Circom | circuit DSL | R1CS-style constraints | snarkjs / Groth16 / PLONK workflows |
| Noir | ZK language | ACIR | Barretenberg / other backends |
| Cairo | Cairo program | CASM plus CairoVM trace / AIR | STARK prover |
| RISC Zero / SP1 | Rust or RISC-V guest | RISC-V program execution trace | zkVM prover |
| Halo2 | circuit API | PLONKish circuit | Halo2 proving system |
| LLZK | outputs from ZK DSLs or circuit systems | MLIR-based ZK circuit IR | analysis, verification, and lowering toward proving targets |

이 표는 단순화된 것이고, 모든 row가 같은 종류의 product를 의미하지도 않습니다. LLZK는 developer-facing proving framework라기보다 intermediate compiler layer에 더 가깝습니다. 오히려 그래서 이 글에서 유용합니다. ZK에도 이런 middle이 실제로 존재한다는 것을 보여주기 때문입니다.

Circom은 개발자를 constraint 가까이에 둡니다. 그만큼 control이 크지만, underconstraining 위험도 직접 드러납니다. Halo2도 다른 PLONKish programming model을 통해 proof target 가까이에 있습니다.

Noir는 source level을 더 끌어올립니다. 개발자는 일반 프로그램에 가까운 코드를 작성하고, ACIR가 language와 proving backend 사이의 boundary 역할을 합니다. 이 boundary는 relation을 analyze, optimize, retarget할 수 있는 위치를 만들어줍니다.

Cairo, RISC Zero, SP1은 execution trace 쪽에 더 가깝습니다. 개발자는 프로그램을 작성하고, 시스템은 machine을 실행하며, proof는 그 machine execution이 valid하다는 것에 관한 것이 됩니다. 이 방식은 ZK를 일반 programming처럼 느끼게 만들 수 있지만, proof-oriented target이 사라지는 것은 아닙니다. target이 VM semantics, trace constraint, program identity, public output, prover cost 쪽으로 이동하는 것입니다.

그래서 유용한 질문은 "어떤 도구가 compiler를 가지고 있는가?"가 아닙니다. 모두 어딘가에 compiler machinery를 가지고 있습니다. 더 좋은 질문은 이것입니다. 이 도구는 개발자에게 어디를 생각하게 하는가? constraint인가, IR인가, backend gate와 lookup인가, VM cycle과 public output인가, program identity인가, verifier cost인가?

이 map이 보이면 ZK landscape는 덜 이상해집니다. 시스템들이 달라 보이는 이유는 서로 다른 boundary를 선택했기 때문입니다. 하지만 모두 같은 근본 문제를 풀고 있습니다. source-level intent를 relation으로 바꾸고, witness나 trace를 만들고, 그 relation이 만족되었다는 것을 proof로 만드는 문제입니다.

## 9. Compiler Correctness Is Verifier Trust

ZK에서 compiler correctness는 단순한 구현 디테일이 아닙니다. verifier가 결국 무엇을 믿게 되는지와 직접 연결됩니다.

proving backend가 sound하더라도, 전달받은 relation이 잘못되어 있다면 잘못된 것을 증명할 수 있습니다. "proof가 valid하다"와 "개발자가 의도한 claim이 증명되었다"는 같은 말이 아닙니다. proof는 relation, public input, verifier key, program identity, protocol에 대해 valid합니다. relation이 의도보다 약하다면 proof는 여전히 완벽하게 valid할 수 있습니다.

작은 age 예시로 보면 모양은 이렇습니다.

```text
intended claim: this private age is at least 18
compiled claim: there exists some private age
proof system:   sound
verifier result: confidence in the wrong statement
```

이것은 좁은 의미의 cryptographic failure가 아닙니다. backend는 자신이 약속한 일을 정확히 했을 수 있습니다. compiled relation이 satisfiable하다는 것을 증명한 것입니다. 실패는 그보다 앞에 있습니다. source-level claim이 verifier가 실제로 확인한 relation으로 살아남지 못한 것입니다.

이런 실패는 여러 layer에서 생길 수 있습니다. 개발자가 source-level claim에서 필요한 obligation을 빠뜨릴 수 있습니다. frontend compiler가 source를 잘못된 relation으로 lowering할 수 있습니다. IR pass나 optimizer가 semantic meaning을 가진 constraint를 제거할 수도 있습니다. backend나 verifier implementation 자체에 soundness bug가 있을 수도 있습니다. integration layer가 proof를 잘못된 public input, key, program image, journal, receipt에 묶을 수도 있습니다.

ZK에서는 어느 layer에서 생긴 버그든 verifier-trust bug가 될 수 있습니다. 그 결과는 단순히 실패하는 proof가 아니라, verifier가 잘못된 claim을 믿게 만드는 valid proof일 수 있습니다. 그래서 중요한 질문은 단지 "backend가 이 relation을 증명할 수 있는가?"가 아닙니다. "이 relation이 우리가 증명하려던 claim을 충실하게 표현하는가?"입니다.

formal verification이 점점 중요해지는 이유도 여기에 있습니다. frontend 쪽에서는 IR과 analysis framework가 circuit, constraint, VM semantics를 확인하는 데 도움을 줄 수 있습니다. 필요한 check가 있는가? transformation이 meaning-preserving한가? public input이 올바르게 binding되어 있는가? [LLZK](https://github.com/project-llzk/llzk-lib)는 IR layer에서의 예시이고, [RISC Zero](https://github.com/risc0/risc0-lean4)를 Lean4로 모델링하거나 [SP1 chip constraint를 Lean으로 검증하는 작업](https://github.com/NethermindEth/sp1-fv-poc)은 같은 흐름이 zkVM의 VM/circuit layer에서도 나타난다는 것을 보여줍니다. backend 쪽에서는 [ArkLib](https://github.com/Verified-zkEVM/ArkLib)처럼 Lean으로 SNARK protocol component와 completeness 또는 soundness argument를 formalize하려는 작업도 등장하고 있습니다.

이런 노력들이 cryptography를 대체하는 것은 아닙니다. 오히려 cryptographic guarantee가 어떤 relation에 붙어 있는지를 명확하게 합니다. proof는 cryptographically valid할 수 있지만, 그 proof가 증명한 claim은 개발자가 의도한 claim이 아닐 수 있습니다. 그래서 ZK compiler의 correctness는 verifier의 trust가 올바른 claim에 붙도록 만드는 조건입니다.

## 10. Cryptography Guarantees; Compilation Defines

영지식 증명은 cryptographic object입니다. proof protocol은 private witness를 드러내지 않으면서도 verifier에게 어떤 statement가 증명되었다는 확신을 줍니다.

하지만 practical ZK system은 compiler system이기도 합니다. 개발자는 program, circuit, guest code, DSL을 작성합니다. toolchain은 그것을 relation, constraint, trace, IR, verification artifact로 바꿉니다. prover는 이 compiled object를 witness나 execution trace와 결합합니다. verifier는 public input, key, program identity, receipt 등에 대해 최종 proof를 확인합니다.

이렇게 보면 practical ZK는 단지 어떤 proof system을 선택하는 문제가 아닙니다. program이 relation이 되는 과정, source operation이 constraint가 되는 방식, execution이 witness나 trace가 되는 흐름, frontend/backend boundary가 놓이는 위치, 그리고 각 layer가 만드는 cost와 trust assumption까지 함께 보는 문제입니다.

Cryptography gives the verifier confidence. The compiler stack decides what that confidence attaches to.
