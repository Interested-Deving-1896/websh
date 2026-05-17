//! Built-in homepage.
//!
//! The root URL is an application surface, not a filesystem document reader.
//! Content routes such as `/#/index.html` remain available through the
//! filesystem router.

use leptos::prelude::*;

use crate::app::AppContext;
use crate::features::chrome::SiteChrome;
use crate::render::render_inline_markdown;
use crate::runtime::MountLoadStatus;
use crate::shared::components::markdown::InlineMarkdownView;
use crate::shared::components::{
    IdentifierStrip, MetaRow as SharedMetaRow, MetaTable as SharedMetaTable, SiteContentFrame,
    SiteSurface,
};
use websh_core::domain::VirtualPath;
use websh_core::filesystem::{GlobalFs, RouteFrame};

stylance::import_crate_style!(
    pub(super) css,
    "src/features/home/home.module.css"
);

mod model;
mod sections;
use model::{
    TOC_ITEMS, compact_homepage_date, current_homepage_date, latest_now_date, parse_now_toml,
    recent_items_from_fs, site_last_revised_at, toc_item_meta,
};
use sections::{Acknowledgements, Appendices, PageFooter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootContentReadiness {
    Loading,
    Loaded,
    Failed,
}

fn root_content_readiness(ctx: AppContext) -> RootContentReadiness {
    match ctx.mount_status_for(&VirtualPath::root()) {
        Some(MountLoadStatus::Loaded { .. }) => RootContentReadiness::Loaded,
        Some(MountLoadStatus::Failed { .. }) => RootContentReadiness::Failed,
        Some(MountLoadStatus::Loading { .. }) | None => RootContentReadiness::Loading,
    }
}

#[component]
pub fn HomePage(route: Memo<RouteFrame>) -> impl IntoView {
    view! {
        <SiteSurface class=css::home>
            <SiteChrome route=route />
            <SiteContentFrame class=css::page>
                <HeroHeader />
                <HomepageMetaTable />
                <AbstractSection />
                <TocSection />
                <IntroSection />
                <RecentFeed />
                <Appendices />
                <Acknowledgements />
                <PageFooter />
            </SiteContentFrame>
        </SiteSurface>
    }
}

#[component]
fn HeroHeader() -> impl IntoView {
    let today = current_homepage_date();
    let paper_id = format!("Paper {}", compact_homepage_date(&today));
    let revised = format!("last revised {}", site_last_revised_at().unwrap_or(today));

    view! {
        <IdentifierStrip>
            <span>{paper_id}</span>
            <span>{revised}</span>
        </IdentifierStrip>

        <h1 class=css::title>
            "wonjae.eth"
            <span class=css::tagline>"A Homepage, Formalised"</span>
        </h1>

        <div class=css::authors>
            "Wonjae Choi"<sup class=css::star>"*"</sup>
        </div>
        <div class=css::aff>
            <sup>"*"</sup>" Seoul National University "
            <span class=css::dotSep>" · "</span>
            <a href="mailto:wonjae@snu.ac.kr">"wonjae@snu.ac.kr"</a>
        </div>
    }
}

#[component]
fn HomepageMetaTable() -> impl IntoView {
    view! {
        <SharedMetaTable class=css::meta aria_label="ePrint metadata">
            <SharedMetaRow
                label="Category"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                <span class=css::tag>"cs.CR"</span>
                <span class=css::tag>"cs.PL"</span>
                <span class=css::tag>"cs.DC"</span>
            </SharedMetaRow>
            <SharedMetaRow
                label="Keywords"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                <span class=css::kwFull>"zero-knowledge proofs"</span>
                <span class=css::kwCompact>"zkp"</span>
                ", compilers, Ethereum"
            </SharedMetaRow>
            <SharedMetaRow
                label="Availability"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                <span class=css::availFull>
                    <span class=css::dim>"ens "</span>
                    <a href="https://wonjae.eth.limo">"wonjae.eth"</a>
                </span>
                <a class=css::availCompact href="https://wonjae.eth.limo">
                    <span class=css::dim>"ens"</span>
                </a>
                <span class=css::dotSep>" · "</span>
                <span class=css::availFull>
                    <span class=css::dim>"email "</span>
                    <a href="mailto:wonjae@snu.ac.kr">"wonjae@snu.ac.kr"</a>
                </span>
                <a class=css::availCompact href="mailto:wonjae@snu.ac.kr">
                    <span class=css::dim>"email"</span>
                </a>
                <span class=css::dotSep>" · "</span>
                <span class=css::availFull>
                    <span class=css::dim>"github "</span>
                    <a href="https://github.com/0xwonj">"0xwonj"</a>
                </span>
                <a class=css::availCompact href="https://github.com/0xwonj">
                    <span class=css::dim>"github"</span>
                </a>
                <span class=css::dotSep>" · "</span>
                <span class=css::availFull>
                    <span class=css::dim>"linkedin "</span>
                    <a href="https://www.linkedin.com/in/wonj">"wonjaechoi"</a>
                </span>
                <a class=css::availCompact href="https://www.linkedin.com/in/wonj">
                    <span class=css::dim>"linkedin"</span>
                </a>
            </SharedMetaRow>
            <SharedMetaRow
                label="Status"
                row_class=css::metaRow
                key_class=css::metaKey
                value_class=css::metaValue
            >
                <span class=css::live>"accepting revisions"</span>
            </SharedMetaRow>
        </SharedMetaTable>
    }
}

#[component]
fn AbstractSection() -> impl IntoView {
    view! {
        <h2 class=css::sectionTitle data-n="">"Abstract"</h2>
        <p>
            "We present a personal homepage, formalised. The author is a PhD student working on "
            <em>"zero-knowledge proofs"</em>", "<em>"compiler design"</em>", and "<em>"Ethereum"</em>".
            The site is a virtual filesystem; "<em>"websh"</em>" is the shell that mounts it."
        </p>

        <NowSection />
    }
}

#[component]
fn NowSection() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");
    let now = LocalResource::new(move || {
        let readiness = root_content_readiness(ctx);
        let path = VirtualPath::from_absolute("/now.toml").expect("constant path");
        let should_read = readiness == RootContentReadiness::Loaded
            && ctx.view_global_fs.with(|fs| fs.exists(&path));

        async move {
            if !should_read {
                return None;
            }

            ctx.read_text(&path)
                .await
                .ok()
                .and_then(|body| parse_now_toml(&body).ok())
        }
    });

    view! {
        {move || {
            now.get().flatten().map(|doc| {
                let timestamp = latest_now_date(&doc.items)
                    .map(|date| format!("last touched {date}"))
                    .unwrap_or_default();

                view! {
                    <div class=css::nowInline>
                        <p class=css::nowFormalLead><em>"Now"</em>":"</p>
                        <ul class=css::nowFormal>
                            {doc.items.into_iter().map(|item| {
                                let rendered = render_inline_markdown(&item.text);
                                let rendered = Signal::derive(move || rendered.clone());
                                view! {
                                    <li><InlineMarkdownView rendered=rendered /></li>
                                }
                            }).collect_view()}
                        </ul>
                        <p class=css::ts>{timestamp}</p>
                    </div>
                }
            })
        }}
    }
}

#[component]
fn TocSection() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");

    view! {
        <nav class=css::toc aria-label="Site index">
            <h2 class=css::tocHeading>"Index"</h2>
            <ol>
                {move || {
                    let readiness = root_content_readiness(ctx);
                    ctx.view_global_fs.with(|fs| {
                        TOC_ITEMS.iter().map(|item| {
                            let meta = toc_item_meta_for_readiness(fs, item, readiness);
                            view! {
                                <li>
                                    <a href=item.href>
                                        <span class=css::num>{item.num}</span>
                                        <span class=css::name>{item.name}</span>
                                        <span class=css::leader></span>
                                        <span class=css::pg>{meta}<span class=css::arrow>"→"</span></span>
                                    </a>
                                </li>
                            }
                        }).collect_view()
                    })
                }}
            </ol>
        </nav>
    }
}

#[component]
fn IntroSection() -> impl IntoView {
    view! {
        <h2 id="sec-intro" class=css::sectionTitle data-n="1.">
            "Introduction"<span class=css::loc>"[§1]"</span>
        </h2>
        <p class=css::introLead>
            "The author is the circuit below; this page is its proof transcript. The job is to convince you, without leaking the "
            <em>"witness"</em>", that the "<em>"constraints"</em>" are satisfiable."
        </p>

        <div class=css::protocol>
            <header>
                <b>"Circuit 1 — the author"</b>
                <span><span class=css::tag>"unaudited"</span></span>
            </header>
            <div class=css::protocolBody>
                <pre class=css::line><span class=css::kw>"public"</span>"       Wonjae Choi · PhD @ SNU · Seoul\n\n"<span class=css::kw>"private"</span>"      mood, unfinished drafts, open browser tabs (n ≫ 1)\n\n"<span class=css::kw>"constraints"</span>"  research  ∋ {zkVMs, ZK Compilers, EVM Compilers}\n             toolchain ∋ {Rust, Python, Solidity, LLVM}\n             habits    ∋ {nocturnal, infinite side projects, wasting LLM tokens}\n             output    = /papers ‖ /writing ‖ /projects ‖ /talks ‖ /misc"</pre>
            </div>
            <footer>
                <span>
                    <span class=css::protocolFootWitness>"witness: private · "</span>
                    "completeness ✓ · soundness ?"
                </span>
                <span class=css::protocolFootSetup>"no trusted setup"</span>
            </footer>
        </div>

        <p>"The rest of this site opens commitments to the above."</p>
    }
}

#[component]
fn RecentFeed() -> impl IntoView {
    let ctx = use_context::<AppContext>().expect("AppContext must be provided");
    let recent_items = Memo::new(move |_| {
        if root_content_readiness(ctx) != RootContentReadiness::Loaded {
            return Vec::new();
        }
        ctx.view_global_fs.with(|fs| recent_items_from_fs(fs))
    });

    view! {
        <h2 id="sec-recent" class=css::sectionTitle data-n="2.">
            "Recent"<span class=css::loc>"[§2]"</span>
        </h2>
        <div class=css::feed>
            {move || {
                recent_items
                    .get()
                    .into_iter()
                    .map(|item| {
                        let kind_class = format!("{} {}", css::kind, feed_kind_class(&item.kind));
                        view! {
                            <div class=css::feedRow>
                                <span class=kind_class>{item.kind}</span>
                                <span class=css::date>{item.date}</span>
                                <span class=css::feedTitle><a href=item.href>{item.title}</a></span>
                                <span class=css::feedTag>{item.tag}</span>
                            </div>
                        }
                    })
                    .collect_view()
            }}
        </div>
    }
}

fn toc_item_meta_for_readiness(
    fs: &GlobalFs,
    item: &model::TocItem,
    readiness: RootContentReadiness,
) -> String {
    if !item.is_count_backed() {
        return toc_item_meta(fs, item);
    }

    match readiness {
        RootContentReadiness::Loaded => toc_item_meta(fs, item),
        RootContentReadiness::Failed => "—".to_string(),
        RootContentReadiness::Loading => "…".to_string(),
    }
}

fn feed_kind_class(kind: &str) -> &'static str {
    match kind {
        "paper" => css::kindPaper,
        "project" => css::kindProject,
        "writing" => css::kindWriting,
        "talk" => css::kindTalk,
        _ => "",
    }
}
