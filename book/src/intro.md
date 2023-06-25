# Introduction

Hira is an infrastructure from code tool that allows you to define and specify infrastructure using Rust code.

At its core, Hira is a Rust procedural macro that can manipulate and generate code at compile time. Users write Rust modules and annotate them with the `#[hira]` macro to define a Hira module. Hira reads, processes, and manipulates the user's code at compile time, and creates the desired infrastructure as specified by the user.

While created with cloud infrastructure deployment in mind, Hira is capable of creating and defining any infrastructure, that is to say: Hira is independent of any specific cloud platform.

Hira is unoppinionated with regards to the infrastructure you generate:
- Hira has no concept of what 'deployment' means
- It does not know anything about cloud providers
- Hira does not have any concept of networking

Instead, all advanced functionality of a Hira module is written at the user level. Hira serves as a foundation, offering users low-level and fundamental capabilities that can be utilized by their own modules to implement advanced features. Read on to learn how Hira works, and how to use it. Or [click here](./examples/main.md) to look at some examples.
