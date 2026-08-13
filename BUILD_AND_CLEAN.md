# Standard build and cleanup scripts

The bundled Mailer has been removed; it is not required by the daemon.

Script names are standardized:

- `build_project` builds one platform project.
- `clean_project` removes generated files for one platform project.
- `build_all_projects` builds all projects available on the current host.
- `clean_all_projects` cleans all projects available on the current host.

Every build script prints its required software and suggested installation commands before checking the toolchain.
