use crate::Runtime;
use comtrya_lib::contexts::to_rhai;
use comtrya_lib::manifests::{load, Manifest};
use petgraph::{visit::DfsPostOrder, Graph};
use rhai::Engine;
use std::{collections::HashMap, ops::Deref};
use structopt::StructOpt;
use tracing::{debug, error, info, instrument, span, trace, warn};

#[derive(Clone, Debug, StructOpt)]
pub(crate) struct Apply {
    /// Run a subset of your manifests, comma separated list
    #[structopt(short = "m", long, use_delimiter = true)]
    manifests: Vec<String>,

    /// Performs a dry-run without changing the system
    #[structopt(long)]
    dry_run: bool,

    #[structopt(short = "l", long = "label")]
    pub label: Option<String>,
}

#[instrument(skip(args, runtime))]
pub(crate) fn execute(args: &Apply, runtime: &Runtime) -> anyhow::Result<()> {
    let manifest_path =
        match crate::manifests::resolve(runtime.config.manifest_paths.first().unwrap()) {
            Some(path) => path,
            None => {
                return Err(anyhow::anyhow!(
                    "Manifest location, {:?}, could be resolved",
                    runtime.config.manifest_paths.first().unwrap()
                ))
            }
        };

    trace!(manifests = args.manifests.join(",").deref(),);

    let contexts = &runtime.contexts;

    let manifests = load(manifest_path, contexts);

    // Build DAG
    let mut dag: Graph<Manifest, u32, petgraph::Directed> = Graph::new();

    let manifest_root = Manifest {
        r#where: None,
        root_dir: None,
        dag_index: None,
        name: None,
        depends: vec![],
        actions: vec![],
        ..Default::default()
    };

    let root_index = dag.add_node(manifest_root);

    let manifests: HashMap<String, Manifest> = manifests
        .into_iter()
        .map(|(name, mut manifest)| {
            let abc = dag.add_node(manifest.clone());

            manifest.dag_index = Some(abc);
            dag.add_edge(root_index, abc, 0);

            (name, manifest)
        })
        .collect();

    for (name, manifest) in manifests.iter() {
        manifest.depends.iter().for_each(|d| {
            let m1 = match manifests.get(d) {
                Some(manifest) => manifest,
                None => {
                    error!(message = "Unresolved dependency", dependency = d.as_str());

                    return;
                }
            };

            trace!(
                message = "Dependency Registered",
                from = name.as_str(),
                to = m1.name.clone().unwrap().as_str()
            );

            dag.add_edge(manifest.dag_index.unwrap(), m1.dag_index.unwrap(), 0);
        });
    }

    let clone_m = args.manifests.clone();

    let run_manifests = if args.manifests.is_empty() {
        // No manifests specified on command line, so run everything
        vec![String::from("")]
    } else {
        // Run subset
        manifests
            .keys()
            .filter(|z| clone_m.contains(z))
            .cloned()
            .collect::<Vec<String>>()
    };

    let dry_run = args.dry_run;

    let engine = Engine::new();
    let mut scope = to_rhai(contexts);

    run_manifests.iter().for_each(|m| {
        let start = if m.eq(&String::from("")) {
            root_index
        } else {
            manifests.get(m).unwrap().dag_index.unwrap()
        };

        let mut dfs = DfsPostOrder::new(&dag, start);

        while let Some(visited) = dfs.next(&dag) {
            let m1 = dag.node_weight(visited).unwrap();

            // Root manifest, nothing to do.
            if m1.name.is_none() {
                continue;
            }

            let span_manifest = span!(
                tracing::Level::INFO,
                "",
                manifest = m1.name.clone().unwrap().as_str()
            )
            .entered();

            let mut successful = true;

            if args.label.is_some() {
                let label = args.label.clone().unwrap();

                if !m1.labels.contains(&label) {
                    info!(
                        message = "Skipping manifest, label not found",
                        label = label.as_str()
                    );
                    continue;
                }
            }

            if let Some(where_condition) = &m1.r#where {
                let where_result = match engine.eval_with_scope::<bool>(&mut scope, where_condition)
                {
                    Ok(result) => {
                        debug!(
                            "Result of 'where' condition '{}' -> '{}'",
                            where_condition, result
                        );

                        result
                    }
                    Err(err) => {
                        warn!("'where' condition '{}' failed: {}", where_condition, err);
                        false
                    }
                };

                if !where_result {
                    info!("Skip manifest, because 'where' conditions were false!");
                    span_manifest.exit();
                    return;
                }
            }

            for action in m1.actions.iter() {
                let span_action = span!(tracing::Level::INFO, "", %action).entered();

                let action = action.inner_ref();

                let plan = match action.plan(m1, contexts) {
                    Ok(steps) => steps,
                    Err(err) => {
                        info!("Action failed to get plan: {:?}", err);
                        successful = false;
                        continue;
                    }
                };

                let mut steps = plan
                    .into_iter()
                    .filter(|step| step.do_initializers_allow_us_to_run())
                    .filter(|step| step.atom.plan())
                    .peekable();

                if steps.peek().is_none() {
                    info!("nothing to be done to reconcile action");
                    span_action.exit();
                    return;
                }

                for mut step in steps {
                    if dry_run {
                        continue;
                    }

                    match step.atom.execute() {
                        Ok(_) => (),
                        Err(err) => {
                            debug!("Atom failed to execute: {:?}", err);
                            successful = false;
                            break;
                        }
                    }

                    if !step.do_finalizers_allow_us_to_continue() {
                        debug!("Finalizers won't allow us to continue with this action");
                        successful = false;
                        break;
                    }
                }
                span_action.exit();
            }

            if dry_run {
                span_manifest.exit();
                continue;
            }

            if !successful {
                error!("Failed");
                span_manifest.exit();
                break;
            }

            info!("Completed");
            span_manifest.exit();
        }
    });

    Ok(())
}
