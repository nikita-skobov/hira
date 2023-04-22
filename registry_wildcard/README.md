lol this is a dumb crate. its purpose is just to iterate at compile time over the registry/src/modules directory
and add `pub mod {module_name};` to the modules.rs file. It just makes imports easier so it's not necessary
to edit the modules.rs file every time a new module is written.
