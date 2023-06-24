# Introduction

Hira is an infrastructure from code tool written in Rust and designed to be specified in Rust code.

At its core, Hira is a Rust procedural macro that can manipulate and generate code at compile time. Users write Rust modules and annotate them with the `#[hira]` macro to define a Hira module. Hira reads, processes, and manipulates the user's code at compile time. Because of this, Hira modules are declarative; users write code that tells Hira what they desire, and then Hira creates all the necessary infrastructure at compile time.

While Hira was created with cloud infrastructure deployment in mind, at its core, Hira is just a framework for creating simple reusable and composable modules that offer procedural macro capabilities in a safe manner. Hira is unoppinionated with regards to the infrastructure you generate:
- Hira has no concept of what 'deployment' means
- It does not know anything about cloud providers
- Hira does not have any concept of networking

Instead, all advanced functionality of a Hira module is written at the user level. Hira itself only provides primitive, low-level capabilities that are used by user-provided modules to do any advanced functionality. Read on to learn how Hira works, and how to use it. Or [click here](./examples/main.md) to look at some examples.
