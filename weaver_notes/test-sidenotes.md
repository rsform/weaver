# Weaver: Long-form Writing on AT Protocol

*Or: "Get in kid, we're rebuilding the blogosphere!"*

I grew up, like a lot of people on Bluesky, in the era of the internet where most of your online social interactions took place via text. I had a MySpace account, MSN messenger and Google Chat, I first got on Facebook back when they required a school email to sign up, I had a Tumblr, though not a LiveJournal.[^nostalgia]
[^nostalgia]: Hi Rahaeli. Sorry I was the wrong kind of nerd.

> ![[weaver_photo_med.jpg]]*The namesake of what I'm building*

Social media in the conventional sense has been in a lot of ways a small part of the story of my time on the internet. The broader independent blogosphere of my teens and early adulthood shaped my worldview, and I was an avid reader and sometime participant there.

## The Blogosphere

I am an atheist in large part because of a blog called Common Sense Atheism.[^csa] Luke's blog was part of a cluster of blogs out of which grew the rationalists, one of, for better or for worse, the most influential intellectual movements of the 21st century.
[^csa]: The author, Luke Muehlhauser, was criticising both Richard Dawkins *and* some Christian apologetics I was familiar with.

I also read blogs like boingboing.net, was a big fan of Cory Doctorow. I figured out I am trans in part because of Thing of Things,[^ozy] a blog by Ozy Frantz, a transmasc person in the broader rationalist and Effective Altruist blogosphere.
[^ozy]: Specifically their piece on the [cluster structure of genderspace](https://thingofthings.wordpress.com/2017/05/05/the-cluster-structure-of-genderspace/).

One thing these all have in common is length. Part of the reason I only really got onto Twitter in 2020 or so was because the concept of microblogging, of having to fit your thoughts into such a small package, baffled me for ages.[^irony]
[^irony]: Amusingly I now think that being on Twitter and now Bluesky made me a better writer. Restrictions breed creativity, after all.

{.aside}
> **On Platform Decay**
> Through all of this I was never really satisfied with the options that were out there for long-form writing. Wordpress required too much setup. Tumblr's system for comments remains insane. Hosting my own seemed like too much money to burn on something nobody might read.

But at the same time, Substack's success proves that there is very much a desire for long-form writing, enough that people will pay for it, and that investors will back it. There are thoughts and forms of writing that you simply cannot fit into a post or even a thread of posts.

Plus, I'm loathe to enable a centralised platform like Substack where the owners are unfortunately friendly to fascists.[^substack]
[^substack]: I am very much a fan of freedom of expression. I'm not so much a fan of paying money to Nazis.

That's where the `at://` protocol and Weaver comes in.

## The Pitch

Weaver is designed to be a highly flexible platform for medium and long-form writing on atproto.[^namesake] I was inspired by how weaver birds build their own homes, and by the notebooks, physical and virtual, that I create in the course of my work.
[^namesake]: The weaver bird builds intricate, self-contained homes—seemed fitting for a platform about owning your writing.

The initial proof-of-concept is essentially a static site generator, able to turn a Markdown text file or a folder of Markdown files into a static "notebook" site. The intermediate goal is an elegant and intuitive writing platform with collaborative editing and straightforward, immediate publishing via a web-app.

{.aside}
> **The Ultimate Goal**
>
> Build a platform suitable for professional writers and journalists, an open alternative to platforms like Substack, with ways for readers to support writers, all on the `at://` protocol.

## How It Works

Weaver works on a concept of notebooks with entries, which can be grouped into pages or chapters. They can have multiple attributed authors. You can tear out a metaphorical page and stick it in another notebook.

You own what you write.[^ownership] And once collaborative editing is in, collaborative work will be resilient against deletion by one author. They can delete their notebook or even their account, but what you write will be safe.
[^ownership]: Technically you can include entries you don't control in your notebooks, although this isn't a supported mode—it's about *your* ownership of *your* words.

Entries are Markdown text—specifically, an extension on the Obsidian flavour of Markdown.[^markdown] They support additional embed types, including atproto record embeds and other markdown documents, as well as resizable images.
[^markdown]: I forked the popular rust markdown processing library `pulldown-cmark` because it had limited extensibility along the axes I wanted—custom syntax extensions to support Obsidian's Markdown flavour and additional useful features, like some of the ones on show here!

## Why Rust?

As to why I'm writing it in Rust (and currently zero Typescript) as opposed to Go and Typescript? Well it comes down to familiarity. Rust isn't necessarily anyone's first choice in a vacuum for a web-native programming language, but it works quite well as one. I can share the vast majority of the protocol code, as well as the markdown rendering engine, between front and back end, with few if any compromises on performance, save a larger bundle size due to the nature of WebAssembly.

{.aside}
> **On Interoperability**
>
> The `at://` protocol, while it was developed in concert with a microblogging app, is actually pretty damn good for "macroblogging" too. Weaver's app server can display Whitewind posts. With effort, it can faithfully render Leaflet posts. It doesn't care what app your profile is on.

## Evolution

Weaver is therefore very much an evolving thing. It will always have and support the proof-of-concept workflow as a first-class citizen. That's part of the benefit of building this on atproto.

If I screw this up, not too hard for someone else to pick up the torch and continue.[^open]
[^open]: This is the traditional footnote, at the end, because sometimes you want your citations at the bottom of the page rather than in the margins.
