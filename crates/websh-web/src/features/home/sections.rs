//! Homepage appendices and acknowledgements (Appendix A/B + Acks + Footer).

use gloo_timers::callback::Timeout;
use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::config::{APP_NAME, APP_VERSION};
use crate::platform::breakpoints::{BP_SM, use_min_width};
use crate::shared::components::{AttestationSigFooter, MonoOverflow, MonoTone, MonoValue};
use websh_core::crypto::ack::{
    AckMembershipProof, AckReceipt, normalize_ack_name, public_proof_for_name, short_hash,
    verify_private_receipt,
};
use websh_core::crypto::pgp::pretty_fingerprint;

use super::css;

#[derive(Clone, Debug, Default)]
struct AckResult {
    message: String,
    proof: Option<AckMembershipProof>,
    included: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AckMathFormula {
    Set,
    RootOfSet,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CopyStatus {
    #[default]
    Idle,
    Copying,
    Copied,
    Failed,
}

#[derive(Clone, Debug, thiserror::Error)]
enum ClipboardError {
    #[error("window not available")]
    NoWindow,
    #[error("clipboard write failed: {message}")]
    Write { message: String },
}

#[component]
pub(super) fn Appendices() -> impl IntoView {
    view! {
        <PublicKeyAppendix />
        <ShellAppendix />
    }
}

#[component]
fn PublicKeyAppendix() -> impl IntoView {
    let (copy_status, set_copy_status) = signal(CopyStatus::Idle);
    let copy_key = move |_| {
        set_copy_status.set(CopyStatus::Copying);
        spawn_local(async move {
            let result = copy_to_clipboard(websh_site::PUBLIC_KEY_BLOCK).await;
            set_copy_status.set(if result.is_ok() {
                CopyStatus::Copied
            } else {
                CopyStatus::Failed
            });
            Timeout::new(1600, move || set_copy_status.set(CopyStatus::Idle)).forget();
        });
    };
    let copy_label = move || match copy_status.get() {
        CopyStatus::Idle => "copy",
        CopyStatus::Copying => "copying",
        CopyStatus::Copied => "copied",
        CopyStatus::Failed => "failed",
    };
    let copy_disabled = move || copy_status.get() == CopyStatus::Copying;
    let copy_live = move || match copy_status.get() {
        CopyStatus::Copied => "Public key copied",
        CopyStatus::Failed => "Copy failed",
        _ => "",
    };

    view! {
        <details class=css::appendix id="appendix-a">
            <summary><h2 class=css::sectionTitle data-n="A.">"Appendix A · Public Key"<span class=css::loc>"[§A]"</span></h2></summary>
            <p>
                "OpenPGP key for "<em>"Wonjae Choi <wonjae@snu.ac.kr>"</em>". Use it to send encrypted mail or verify signatures. Rotation: when it annoys me."
            </p>
            <p class=css::footnote>
                "Fingerprint: "<span class=css::fp>{pretty_fingerprint(websh_site::EXPECTED_PGP_FINGERPRINT)}</span>
            </p>
            <pre class=css::keyblock aria-label="PGP public key block">
                {websh_site::PUBLIC_KEY_BLOCK.lines().map(|line| {
                    let line_class = if public_key_block_header_line(line) {
                        css::keyHeader
                    } else {
                        css::keyBody
                    };
                    let (plain, accent) = split_public_key_checksum_tail(line);
                    view! {
                        <span class=line_class>{plain}<span class=css::fp>{accent}</span></span>
                        "\n"
                    }
                }).collect_view()}<button
                    class=css::copy
                    type="button"
                    on:click=copy_key
                    prop:disabled=copy_disabled
                >
                {copy_label}
            </button></pre>
            <span
                role="status"
                aria-live="polite"
                style="position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0;"
            >
                {copy_live}
            </span>
            <p class=css::footnote>
                "Also reachable via the virtual filesystem at "<a href="#/keys/wonjae.asc">"/keys/wonjae.asc"</a>"."
            </p>
        </details>
    }
}

async fn copy_to_clipboard(text: &str) -> Result<(), ClipboardError> {
    let Some(window) = web_sys::window() else {
        return Err(ClipboardError::NoWindow);
    };
    let clipboard = window.navigator().clipboard();
    JsFuture::from(clipboard.write_text(text))
        .await
        .map(|_| ())
        .map_err(|error| ClipboardError::Write {
            message: error
                .as_string()
                .unwrap_or_else(|| "clipboard write failed".to_string()),
        })
}

fn public_key_block_header_line(line: &str) -> bool {
    matches!(
        line,
        "-----BEGIN PGP PUBLIC KEY BLOCK-----" | "-----END PGP PUBLIC KEY BLOCK-----"
    )
}

fn split_public_key_checksum_tail(line: &str) -> (&str, &str) {
    if !line.starts_with('=') {
        return (line, "");
    }

    let Some((split_at, _)) = line.char_indices().rev().nth(3) else {
        return ("", line);
    };
    (&line[..split_at], &line[split_at..])
}

#[component]
fn ShellAppendix() -> impl IntoView {
    let (term_collapsed, set_term_collapsed) = signal(false);
    let banner = websh_site::ASCII_BANNER
        .strip_prefix('\n')
        .unwrap_or(websh_site::ASCII_BANNER)
        .trim_end();

    view! {
        <details class=css::appendix id="appendix-b">
            <summary><h2 class=css::sectionTitle data-n="B.">
                "Appendix B · "
                <span class=css::appendixBFull>"Reference Implementation"</span>
                <span class=css::appendixBCompact>"Websh"</span>
                <span class=css::loc>"[§B]"</span>
            </h2></summary>
            <p>
                "Below is a non-interactive transcript of "
                <a href="#/websh" class=css::appendixShellName>"websh"</a>
                ", the browser-resident shell distributed alongside this preprint. The shell backs onto a virtual filesystem in which every section of this page is a file."
            </p>
            <div class=css::term>
                <header class=css::termTopBar aria-label="websh transcript window">
                    <span class=css::termTraffic>
                        <button
                            class=css::termLight
                            data-tone="close"
                            type="button"
                            aria-label="Collapse transcript"
                            aria-controls="appendix-b-transcript"
                            aria-expanded=move || (!term_collapsed.get()).to_string()
                            on:click=move |_| set_term_collapsed.update(|collapsed| *collapsed = !*collapsed)
                        ></button>
                        <button
                            class=css::termLight
                            data-tone="minimize"
                            type="button"
                            aria-label="Minimize transcript"
                            aria-controls="appendix-b-transcript"
                            aria-expanded=move || (!term_collapsed.get()).to_string()
                            on:click=move |_| set_term_collapsed.update(|collapsed| *collapsed = !*collapsed)
                        ></button>
                        <a
                            class=css::termLight
                            data-tone="zoom"
                            href="#/websh"
                            aria-label="Open websh"
                        ></a>
                    </span>
                    <span class=css::termTitle>{format!("websh v{APP_VERSION}")}</span>
                </header>
                <Show when=move || !term_collapsed.get()>
                    <div class=css::termBody id="appendix-b-transcript">
                        <div class=css::termLine><span class=css::out>{format!("[   0.000] Booting websh kernel v{APP_VERSION}")}</span></div>
                        <div class=css::termLine><span class=css::okOut>"[   0.030] WASM runtime initialized"</span></div>
                        <div class=css::termLine><span class=css::out>"[   0.053] Mounting filesystems..."</span></div>
                        <div class=css::termLine><span class=css::okOut>"[   0.096] Total: 9 files mounted"</span></div>
                        <div class=css::termLine><span class=css::warnOut>"[   0.139] Initializing Terminal mode"</span></div>
                        <div class=css::termLine><span class=css::okOut>{format!("[   0.179] Boot complete. Welcome to {APP_NAME}")}</span></div>
                        <div class=css::termGap></div>
                        <pre class=css::termBanner>{banner}</pre>
                        <div class=css::termLine><span class=css::warnOut>"Zero-Knowledge Proofs | Compiler Design | Ethereum"</span></div>
                        <div class=css::termGap></div>
                        <div class=css::termLine><span class=css::out>"Tips:"</span></div>
                        <div class=css::termLine><span class=css::out>"  - Type 'help' for available commands"</span></div>
                        <div class=css::termLine><span class=css::out>"  - Use the archive bar to jump between home, ledger, and websh"</span></div>
                        <div class=css::termGap></div>
                        <div class=css::commandLine>
                            <span class=css::prompt>{format!("guest@{APP_NAME}:~")}</span>
                            <span class=css::separator>"$ "</span>
                            <span class=css::cmd>"ls"</span>
                        </div>
                        <div class=css::listEntry><span class=css::dir>"keys/"</span><span class=css::out>"keys"</span></div>
                        <div class=css::listEntry><span class=css::dir>"papers/"</span><span class=css::out>"papers"</span></div>
                        <div class=css::listEntry><span class=css::dir>"projects/"</span><span class=css::out>"projects"</span></div>
                        <div class=css::listEntry><span class=css::dir>"talks/"</span><span class=css::out>"talks"</span></div>
                        <div class=css::listEntry><span class=css::dir>"writing/"</span><span class=css::out>"writing"</span></div>
                        <div class=css::listEntry><span class=css::dir>".websh/"</span><span class=css::out>".websh"</span></div>
                        <div class=css::listEntry><span class=css::file>"now.toml"</span><span class=css::out>"now"</span></div>
                        <div class=css::termGap></div>
                        <div class=css::commandLine>
                            <span class=css::prompt>{format!("guest@{APP_NAME}:~")}</span>
                            <span class=css::separator>"$ "</span>
                            <span class=css::cmd>"help | grep theme"</span>
                        </div>
                        <div class=css::termLine><span class=css::out>"    theme [name]  List or set palette"</span></div>
                        <div class=css::termGap></div>
                        <div class=css::inputLine>
                            <span class=css::prompt>{format!("guest@{APP_NAME}:~")}</span>
                            <span class=css::separator>"$ "</span>
                            <span class=css::cursor></span>
                        </div>
                    </div>
                </Show>
            </div>
        </details>
    }
}

#[component]
pub(super) fn Acknowledgements() -> impl IntoView {
    let artifact = websh_site::ack_artifact().expect("homepage ACK artifact must parse");
    let combined_root = artifact.combined_root.clone();
    let depth = ack_public_depth(artifact.public.count);
    let ack_count = artifact.public.count;
    let (ack_input, set_ack_input) = signal(String::new());
    let (ack_result, set_ack_result) = signal(AckResult::default());
    let above_sm = use_min_width(BP_SM);
    let ack_placeholder = move || {
        if above_sm.get() {
            "enter a public name or paste a private receipt"
        } else {
            "enter name or receipt"
        }
    };

    let public_artifact = artifact.clone();
    let run_ack_check = Callback::new(move |_: ()| {
        let raw = ack_input.get();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            set_ack_result.set(AckResult::default());
            return;
        }

        if looks_like_ack_receipt(trimmed) {
            let receipt = match serde_json::from_str::<AckReceipt>(trimmed) {
                Ok(receipt) => receipt,
                Err(error) => {
                    set_ack_result.set(AckResult {
                        message: format!("✗ receipt JSON parse failed · {error}"),
                        proof: None,
                        included: false,
                    });
                    return;
                }
            };

            match verify_private_receipt(&public_artifact, &receipt) {
                Ok(verification) => set_ack_result.set(AckResult {
                    message: format!(
                        "✓ private acknowledgement receipt · name committed privately · root {}",
                        short_hash(&verification.combined_root)
                    ),
                    proof: None,
                    included: true,
                }),
                Err(error) => set_ack_result.set(AckResult {
                    message: format!("✗ private acknowledgement receipt invalid · {error}"),
                    proof: None,
                    included: false,
                }),
            }
            return;
        }

        if normalize_ack_name(&raw).is_empty() {
            set_ack_result.set(AckResult::default());
            return;
        }

        let proof = match public_proof_for_name(&public_artifact, &raw) {
            Ok(Some(proof)) => proof,
            Ok(None) => {
                set_ack_result.set(AckResult {
                    message: format!(
                        "✗ no public acknowledgement for \"{}\" · paste a private receipt if this entry is private.",
                        normalize_ack_name(&raw)
                    ),
                    proof: None,
                    included: false,
                });
                return;
            }
            Err(error) => {
                set_ack_result.set(AckResult {
                    message: format!("✗ commitment error · {error}"),
                    proof: None,
                    included: false,
                });
                return;
            }
        };

        let idx = proof.idx;
        let side_path = proof.side_path();
        set_ack_result.set(AckResult {
            message: format!("✓ included · leaf {idx} · path: {side_path}"),
            proof: Some(proof),
            included: true,
        });
    });

    view! {
        <h2 class=css::sectionTitle data-n="">"Acknowledgements"</h2>
        <p>
            "The author thanks the set "
            <AckMathInline formula=AckMathFormula::Set />
            " whose membership is succinctly attested by the commitment "
            <AckMathInline formula=AckMathFormula::RootOfSet />
            " below. If your name is in "
            <AckMathInline formula=AckMathFormula::Set />
            ", you may verify it; if it is not, this is a soundness bug - please report."
        </p>
        <details class=css::ackMerkle id="ackMerkle">
            <summary>
                <span class=css::lab>"commitment"</span>
                <MonoValue
                    value=combined_root.clone()
                    tone=MonoTone::Hex
                    overflow=MonoOverflow::ResponsiveMiddle {
                        narrow: Some((6, 4)),
                        medium: Some((10, 6)),
                        wide: Some((18, 8)),
                    }
                    title=combined_root
                />
                <span class=css::metaBits>
                    <span class=css::dim>"n"</span>"="{ack_count}" · "
                    <span class=css::dim>"depth"</span>"="{depth}
                    <span class=css::ackHashBit>" · "<span class=css::dim>"hash"</span>"=sha-256"</span>
                </span>
            </summary>
            <div class=css::verify>
                <span class=css::verifyPrompt>"verify ▸"</span>
                <input
                    type="text"
                    placeholder=ack_placeholder
                    autocomplete="off"
                    spellcheck="false"
                    prop:value=move || ack_input.get()
                    on:input=move |ev| set_ack_input.set(event_target_value(&ev))
                    on:keydown=move |ev: ev::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            run_ack_check.run(());
                        }
                    }
                />
                <button type="button" on:click=move |_| run_ack_check.run(())>"verify"</button>
                <span class=move || ack_result.with(verify_result_class)>
                    {move || ack_result.with(|r| r.message.clone())}
                </span>
            </div>
            <Show when=move || ack_result.with(|result| result.proof.is_some())>
                <details class=css::proof>
                    <summary>"view inclusion proof"</summary>
                    <pre class=css::proofBody>
                        {move || ack_result.get().proof.map(|proof| view! { <AckProofView proof=proof /> })}
                    </pre>
                </details>
            </Show>
        </details>

        <p class=css::ackFootnote>
            "Reviewers: please do not reject this homepage."
        </p>
    }
}

#[component]
fn AckMathInline(formula: AckMathFormula) -> impl IntoView {
    match formula {
        AckMathFormula::Set => view! {
            <math class=css::ackInlineMath display="inline" aria-label="set S">
                <mi>"S"</mi>
            </math>
        }
        .into_any(),
        AckMathFormula::RootOfSet => view! {
            <math class=css::ackInlineMath display="inline" aria-label="root of set S">
                <mrow>
                    <mi mathvariant="normal">"root"</mi>
                    <mo>"("</mo>
                    <mi>"S"</mi>
                    <mo>")"</mo>
                </mrow>
            </math>
        }
        .into_any(),
    }
}

#[component]
fn AckProofView(proof: AckMembershipProof) -> impl IntoView {
    let idx = proof.idx;
    let target = proof.target;
    let name = proof.name;
    let leaf_hex = proof.leaf_hex;
    let steps = proof.steps;
    let recomputed_hex = proof.recomputed_hex;
    let committed_hex = proof.committed_hex;
    let verified = proof.verified;

    let hash_overflow = || MonoOverflow::ResponsiveMiddle {
        narrow: Some((12, 6)),
        medium: Some((24, 8)),
        wide: None,
    };

    view! {
        <span class=css::leafLine>
            <span class=css::proofK>{format!("leaf[{idx}]")}</span>
            {format!("     = sha256(\"websh.ack.public.leaf.v1\" || len || \"{target}\")")}
        </span>
        "            = "
        <MonoValue value=leaf_hex tone=MonoTone::Hex overflow=hash_overflow() />
        "\n\n"
        {steps.into_iter().map(|step| view! {
            <span class=css::proofK>{format!("step {}", step.number)}</span>
            "     sibling."
            <span class=css::proofK>{step.side}</span>
            " = "
            <MonoValue value=step.sibling_hex tone=MonoTone::Hex overflow=hash_overflow() />
            "\n           parent    = "
            <MonoValue value=step.parent_hex tone=MonoTone::Hex overflow=hash_overflow() />
            "\n"
        }).collect_view()}
        "\nrecomputed = "
        <MonoValue value=recomputed_hex tone=MonoTone::Hex overflow=hash_overflow() />
        "\ncommitted  = "
        <MonoValue value=committed_hex tone=MonoTone::Hex overflow=hash_overflow() />
        "\n\n"
        <span class=css::proofH>
            {if verified {
                format!("✓ verified · \"{name}\" ∈ commitment")
            } else {
                "✗ root mismatch (this should not happen)".to_string()
            }}
        </span>
    }
}

#[component]
pub(super) fn PageFooter() -> impl IntoView {
    view! {
        <AttestationSigFooter
            route=Signal::derive(|| "/".to_string())
            show_pending=Signal::derive(|| true)
            colophon=true
        />
    }
}

fn verify_result_class(result: &AckResult) -> String {
    if result.message.is_empty() {
        css::verifyResult.to_string()
    } else if result.included {
        format!("{} {}", css::verifyResult, css::verifyOk)
    } else {
        format!("{} {}", css::verifyResult, css::verifyNo)
    }
}

fn looks_like_ack_receipt(input: &str) -> bool {
    input.starts_with('{') || input.contains("\"websh.ack.private.receipt.v1\"")
}

fn ack_public_depth(count: usize) -> usize {
    if count <= 1 {
        return 0;
    }
    usize::BITS as usize - (count - 1).leading_zeros() as usize
}
