# tein

> *branch and rune-stick*

**tein** is an embeddable Scheme interpreter for Rust, built on [Chibi-Scheme](https://github.com/ashinn/chibi-scheme). it provides a safe, ergonomic rust api for embedding r7rs scheme in your applications.

## etymology

from old norse **tein** (teinn):
1. **branch, twig** — like the branches of an abstract syntax tree
2. **rune-stick** — carved wood used to write magical symbols

the name captures both the tree-like structure of code and the symbolic, homoiconic nature of scheme's s-expressions. code as data, data as runes, runes as branches in the world-tree of computation.

## status

🌱 early development — the sapling is just beginning to grow

## philosophy

tein embraces:
- **homoiconicity** — code is data, data is code
- **safety** — rust's ownership model protecting scheme evaluation
- **simplicity** — r7rs compliance without unnecessary complexity
- **elegance** — clean api that feels natural in both rust and scheme

designed for:
- configuration files (s-expressions >> toml/yaml)
- scripting and extension languages
- agent-friendly dsls (ai-readable, ai-writable)
- embedding lisp where you need it

## why scheme? why chibi?

**scheme** gives you:
- homoiconic syntax (trivial to parse and manipulate)
- proper tail calls and continuations
- hygienic macros for extensibility
- minimalist elegance

**chibi** specifically:
- tiny footprint (~200kb)
- zero external dependencies
- full r7rs-small compliance
- designed explicitly for embedding

## why tein?

because embedding scheme in rust shouldn't require wrestling with raw ffi. tein handles the unsafe bits and gives you a safe, idiomatic rust api.

---

*carved with care, grown with intention*
