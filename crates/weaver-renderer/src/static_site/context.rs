use crate::static_site::{StaticSiteOptions, default_md_options};
use crate::theme::Theme;
use crate::{Frontmatter, NotebookContext};
use dashmap::DashMap;
use markdown_weaver::{CowStr, EmbedType, Tag, WeaverAttributes};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use syntect::parsing::SyntaxSet;
use weaver_common::{
    aturi_to_http,
    jacquard::{
        client::{Agent, AgentSession, AgentSessionExt},
        prelude::*,
        types::blob::MimeType,
    },
};
use yaml_rust2::Yaml;

#[derive(Debug, Clone)]
pub enum KaTeXSource {
    Cdn,
    Local(PathBuf),
}

pub struct StaticSiteContext<A: AgentSession> {
    pub options: StaticSiteOptions,
    pub md_options: markdown_weaver::Options,
    pub bsky_appview: CowStr<'static>,
    pub root: PathBuf,
    pub destination: PathBuf,
    pub start_at: PathBuf,
    pub frontmatter: Arc<DashMap<PathBuf, Frontmatter>>,
    pub dir_contents: Option<Arc<[PathBuf]>>,
    reference_map: Arc<DashMap<CowStr<'static>, PathBuf>>,
    pub titles: Arc<DashMap<PathBuf, CowStr<'static>>>,
    pub position: usize,
    pub client: Option<reqwest::Client>,
    agent: Option<Arc<Agent<A>>>,

    pub theme: Option<Arc<Theme>>,
    pub katex_source: Option<KaTeXSource>,
    pub syntax_set: Arc<SyntaxSet>,
    pub index_file: Option<PathBuf>,
}

impl<A: AgentSession> Clone for StaticSiteContext<A> {
    fn clone(&self) -> Self {
        Self {
            options: self.options.clone(),
            md_options: self.md_options.clone(),
            bsky_appview: self.bsky_appview.clone(),
            root: self.root.clone(),
            destination: self.destination.clone(),
            start_at: self.start_at.clone(),
            frontmatter: self.frontmatter.clone(),
            dir_contents: self.dir_contents.clone(),
            reference_map: self.reference_map.clone(),
            titles: self.titles.clone(),
            position: self.position.clone(),
            client: self.client.clone(),
            agent: self.agent.clone(),
            theme: self.theme.clone(),
            katex_source: self.katex_source.clone(),
            syntax_set: self.syntax_set.clone(),
            index_file: self.index_file.clone(),
        }
    }
}

impl<A: AgentSession> StaticSiteContext<A> {
    pub fn clone_with_dir_contents(&self, dir_contents: &[PathBuf]) -> Self {
        Self {
            start_at: self.start_at.clone(),
            root: self.root.clone(),
            bsky_appview: self.bsky_appview.clone(),
            options: self.options.clone(),
            md_options: self.md_options.clone(),
            frontmatter: self.frontmatter.clone(),
            dir_contents: Some(Arc::from(dir_contents)),
            destination: self.destination.clone(),
            reference_map: self.reference_map.clone(),
            titles: self.titles.clone(),
            position: self.position,
            client: self.client.clone(),
            agent: self.agent.clone(),
            theme: self.theme.clone(),
            katex_source: self.katex_source.clone(),
            syntax_set: self.syntax_set.clone(),
            index_file: self.index_file.clone(),
        }
    }

    pub fn clone_with_path(&self, path: impl AsRef<Path>) -> Self {
        let position = if let Some(dir_contents) = &self.dir_contents {
            dir_contents
                .iter()
                .position(|p| p == path.as_ref())
                .unwrap_or(0)
        } else {
            0
        };
        Self {
            start_at: self.start_at.clone(),
            root: self.root.clone(),
            bsky_appview: self.bsky_appview.clone(),
            options: self.options.clone(),
            md_options: self.md_options.clone(),
            frontmatter: self.frontmatter.clone(),
            dir_contents: self.dir_contents.clone(),
            destination: self.destination.clone(),
            reference_map: self.reference_map.clone(),
            titles: self.titles.clone(),
            position,
            client: Some(reqwest::Client::default()),
            agent: self.agent.clone(),
            theme: self.theme.clone(),
            katex_source: self.katex_source.clone(),
            syntax_set: self.syntax_set.clone(),
            index_file: self.index_file.clone(),
        }
    }
    pub fn new(root: PathBuf, destination: PathBuf, session: Option<A>) -> Self {
        Self {
            start_at: root.clone(),
            root,
            bsky_appview: CowStr::Borrowed("deer.social"),
            options: StaticSiteOptions::default(),
            md_options: default_md_options(),
            frontmatter: Arc::new(DashMap::new()),
            dir_contents: None,
            destination,
            reference_map: Arc::new(DashMap::new()),
            titles: Arc::new(DashMap::new()),
            position: 0,
            client: Some(reqwest::Client::default()),
            agent: session.map(|session| Arc::new(Agent::new(session))),
            theme: None,
            katex_source: None,
            syntax_set: Arc::new(SyntaxSet::load_defaults_newlines()),
            index_file: None,
        }
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = Some(Arc::new(theme));
        self
    }

    pub fn current_path(&self) -> &PathBuf {
        if let Some(dir_contents) = &self.dir_contents {
            &dir_contents[self.position]
        } else {
            &self.start_at
        }
    }

    #[inline]
    pub fn handle_link_aturi<'s>(&self, link: Tag<'s>) -> Tag<'s> {
        let link = crate::utils::resolve_at_ident_or_uri(&link, &self.bsky_appview);
        self.handle_link_normal(link)
    }

    pub async fn handle_embed_aturi<'s>(&self, embed: Tag<'s>) -> Tag<'s> {
        match &embed {
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                if dest_url.starts_with("at://") {
                    let width = if let Some(attrs) = attrs {
                        let mut width = 600;
                        for attr in &attrs.attrs {
                            if attr.0 == CowStr::Borrowed("width".into()) {
                                width = attr.1.parse::<usize>().unwrap_or(600);
                                break;
                            }
                        }
                        width
                    } else {
                        600
                    };
                    let html = if let Some(client) = &self.client {
                        if let Ok(resp) = client
                            .get("https://embed.bsky.app/oembed")
                            .query(&[
                                ("url", dest_url.clone().into_string()),
                                ("maxwidth", width.to_string()),
                            ])
                            .send()
                            .await
                        {
                            resp.text().await.ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(html) = html {
                        let link = aturi_to_http(&dest_url, &self.bsky_appview)
                            .expect("assuming the at-uri is valid rn");
                        let mut attrs = if let Some(attrs) = attrs {
                            attrs.clone()
                        } else {
                            WeaverAttributes {
                                classes: vec![],
                                attrs: vec![],
                            }
                        };
                        attrs.attrs.push(("content".into(), html.into()));
                        Tag::Embed {
                            embed_type: EmbedType::Comments, // change this when i update markdown-weaver
                            dest_url: link.into_static(),
                            title: title.clone(),
                            id: id.clone(),
                            attrs: Some(attrs),
                        }
                    } else {
                        self.handle_embed_normal(embed).await
                    }
                } else {
                    self.handle_embed_normal(embed).await
                }
            }
            _ => embed,
        }
    }

    pub async fn handle_embed_normal<'s>(&self, embed: Tag<'s>) -> Tag<'s> {
        // This option will REALLY slow down iteration over events.
        if self.options.contains(StaticSiteOptions::INLINE_EMBEDS) {
            match &embed {
                Tag::Embed {
                    embed_type: _,
                    dest_url,
                    title,
                    id,
                    attrs,
                } => {
                    let mut attrs = if let Some(attrs) = attrs {
                        attrs.clone()
                    } else {
                        WeaverAttributes {
                            classes: vec![],
                            attrs: vec![],
                        }
                    };
                    let contents = if crate::utils::is_local_path(dest_url) {
                        let file_path = if crate::utils::is_relative_link(dest_url) {
                            let root_path = self.root.clone();
                            root_path.join(Path::new(&dest_url as &str))
                        } else {
                            PathBuf::from(&dest_url as &str)
                        };
                        crate::utils::inline_file(&file_path).await
                    } else if let Some(client) = &self.client {
                        if let Ok(resp) = client.get(dest_url.clone().into_string()).send().await {
                            resp.text().await.ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(contents) = contents {
                        attrs.attrs.push(("content".into(), contents.into()));
                        Tag::Embed {
                            embed_type: EmbedType::Markdown, // change this when i update markdown-weaver
                            dest_url: dest_url.clone(),
                            title: title.clone(),
                            id: id.clone(),
                            attrs: Some(attrs),
                        }
                    } else {
                        embed
                    }
                }
                _ => embed,
            }
        } else {
            embed
        }
    }

    /// This is a no-op for the static site renderer currently.
    #[inline]
    pub fn handle_link_normal<'s>(&self, link: Tag<'s>) -> Tag<'s> {
        link
    }

    /// This is a no-op for the static site renderer currently.
    #[inline]
    pub fn handle_image_normal<'s>(&self, image: Tag<'s>) -> Tag<'s> {
        image
    }

    pub fn set_options(&mut self, options: StaticSiteOptions) {
        self.options = options;
    }
}

impl<A: AgentSession + IdentityResolver> StaticSiteContext<A> {
    /// TODO: rework this a bit, to not just do the same thing as whitewind
    /// (also need to make a record to refer to them) that being said, doing
    /// this with the static site renderer isn't *really* the standard workflow
    pub async fn upload_image<'s>(&self, image: Tag<'s>) -> Tag<'s> {
        if let Some(agent) = &self.agent {
            match &image {
                Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                    attrs,
                } => {
                    if crate::utils::is_local_path(&dest_url) {
                        let root_path = self.root.clone();
                        let file_path = root_path.join(Path::new(&dest_url as &str));
                        if let Ok(image_data) = std::fs::read(&file_path) {
                            if let Ok(blob) = agent
                                .upload_blob(image_data, MimeType::new_static("image/jpg"))
                                .await
                            {
                                let (did, _) = agent.info().await.unwrap();
                                let url = weaver_common::blob_url(
                                    &did,
                                    agent.endpoint().await.as_str(),
                                    &blob.r#ref.0,
                                );
                                return Tag::Image {
                                    link_type: *link_type,
                                    dest_url: url.into(),
                                    title: title.clone(),
                                    id: id.clone(),
                                    attrs: attrs.clone(),
                                };
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        image
    }
}

impl<A: AgentSession + IdentityResolver> NotebookContext for StaticSiteContext<A> {
    fn set_entry_title(&self, title: CowStr<'_>) {
        let path = self.current_path();
        self.titles
            .insert(path.clone(), title.clone().into_static());
        self.frontmatter.get_mut(path).map(|frontmatter| {
            if let Ok(mut yaml) = frontmatter.yaml.write() {
                if yaml.get(0).is_some_and(|y| y.is_hash()) {
                    let map = yaml.get_mut(0).unwrap().as_mut_hash().unwrap();
                    map.insert(
                        Yaml::String("title".into()),
                        Yaml::String(title.into_static().into()),
                    );
                }
            }
        });
    }
    fn entry_title(&self) -> CowStr<'_> {
        let path = self.current_path();
        self.titles.get(path).unwrap().clone()
    }

    fn frontmatter(&self) -> Frontmatter {
        let path = self.current_path();
        self.frontmatter.get(path).unwrap().value().clone()
    }

    fn set_frontmatter(&self, frontmatter: Frontmatter) {
        let path = self.current_path();
        self.frontmatter.insert(path.clone(), frontmatter);
    }

    async fn handle_link<'s>(&self, link: Tag<'s>) -> Tag<'s> {
        bitflags::bitflags_match!(self.options, {
            // Split this somehow or just combine the options
            StaticSiteOptions::RESOLVE_AT_URIS | StaticSiteOptions::RESOLVE_AT_IDENTIFIERS => {
                self.handle_link_aturi(link)
            }
            _ => match &link {
                Tag::Link { link_type, dest_url, title, id } => {
                    if self.options.contains(StaticSiteOptions::FLATTEN_STRUCTURE) {
                        let (parent, filename) = crate::utils::flatten_dir_to_just_one_parent(&dest_url);
                        let dest_url = if crate::utils::is_relative_link(&dest_url)
                            && self.options.contains(StaticSiteOptions::CREATE_CHAPTERS_BY_DIRECTORY) {
                            if !parent.is_empty() {
                                CowStr::Boxed(format!("./{}/{}", parent, filename).into_boxed_str())
                            } else {
                                CowStr::Boxed(format!("./{}", filename).into_boxed_str())
                            }
                        } else {
                            CowStr::Boxed(format!("./entry/{}", filename).into_boxed_str())
                        };
                        Tag::Link {
                            link_type: *link_type,
                            dest_url,
                            title: title.clone(),
                            id: id.clone(),
                        }
                    } else {
                        link

                    }
                },
                _ => link,
            }
        })
    }

    async fn handle_image<'s>(&self, image: Tag<'s>) -> Tag<'s> {
        if self.options.contains(StaticSiteOptions::UPLOAD_BLOBS) {
            self.upload_image(image).await
        } else {
            self.handle_image_normal(image)
        }
    }

    async fn handle_embed<'s>(&self, embed: Tag<'s>) -> Tag<'s> {
        if self.options.contains(StaticSiteOptions::RESOLVE_AT_URIS)
            || self.options.contains(StaticSiteOptions::ADD_LINK_PREVIEWS)
        {
            self.handle_embed_aturi(embed).await
        } else {
            self.handle_embed_normal(embed).await
        }
    }

    fn handle_reference(&self, reference: CowStr<'_>) -> CowStr<'_> {
        let reference = reference.into_static();
        if let Some(reference) = self.reference_map.get(&reference) {
            let path = reference.value().clone();
            CowStr::Boxed(path.to_string_lossy().into_owned().into_boxed_str())
        } else {
            reference
        }
    }

    fn add_reference(&self, reference: CowStr<'_>) {
        let path = self.current_path();
        self.reference_map
            .insert(reference.into_static(), path.clone());
    }
}
