use crate::service::Service;
use std::path::Path;

fn has(base: &Path, name: &str) -> bool {
    base.join(name).exists()
}

fn read_to_string(base: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(base.join(name)).ok()
}

fn python_cmd() -> &'static str {
    if std::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        "python3"
    } else {
        "python"
    }
}

fn package_manager(base: &Path) -> &'static str {
    if has(base, "bun.lockb") {
        "bun"
    } else if has(base, "pnpm-lock.yaml") {
        "pnpm"
    } else if has(base, "yarn.lock") {
        "yarn"
    } else {
        "npm"
    }
}

fn detect_primary(base: &Path) -> Option<Service> {
    if has(base, "package.json") {
        if let Some(content) = read_to_string(base, "package.json") {
            let pm = package_manager(base);
            let has_script = |name: &str| content.contains(&format!("\"{}\"", name));

            if has_script("dev") || has_script("develop") {
                let cmd = if content.contains("\"next\"") {
                    format!("{} run dev", pm)
                } else if content.contains("\"vite\"") {
                    format!("{} run dev", pm)
                } else if content.contains("\"react-scripts\"") {
                    format!("{} start", pm)
                } else if content.contains("\"vue\"") || content.contains("\"@vue\"") {
                    format!("{} run dev", pm)
                } else if content.contains("\"@angular\"") || content.contains("\"angular\"") {
                    "ng serve".to_string()
                } else if content.contains("\"svelte\"") || content.contains("\"@svelte\"") {
                    format!("{} run dev", pm)
                } else if content.contains("\"nuxt\"") {
                    format!("{} run dev", pm)
                } else {
                    format!("{} run dev", pm)
                };
                return Some(Service::new("Dev", &cmd));
            }

            if has_script("start") {
                return Some(Service::new("Dev", &format!("{} start", pm)));
            }
        }
    }

    if has(base, "Cargo.toml") {
        return Some(Service::new("Dev", "cargo run"));
    }

    if has(base, "go.mod") {
        return Some(Service::new("Dev", "go run ."));
    }

    if has(base, "pom.xml") {
        return Some(Service::new("Dev", "mvn spring-boot:run"));
    }

    if has(base, "build.gradle") || has(base, "build.gradle.kts") {
        return Some(Service::new("Dev", "gradle bootRun"));
    }

    if has(base, "manage.py") {
        return Some(Service::new("Dev", &format!("{} manage.py runserver", python_cmd())));
    }

    if has(base, "pyproject.toml") {
        return Some(Service::new("Dev", "uv run"));
    }

    if has(base, "requirements.txt") {
        return Some(Service::new("Dev", &format!("{} main.py", python_cmd())));
    }

    if has(base, "Gemfile") {
        return Some(Service::new("Dev", "bundle exec rails server"));
    }

    if has(base, "mix.exs") {
        return Some(Service::new("Dev", "mix phx.server"));
    }

    if has(base, "composer.json") {
        if has(base, "artisan") {
            return Some(Service::new("Dev", "php artisan serve"));
        }
        return Some(Service::new("Dev", "php -S localhost:8000"));
    }

    if has(base, "Program.cs") || has(base, "Program.fs") {
        return Some(Service::new("Dev", "dotnet run"));
    }

    if has(base, "main.py") {
        if let Some(content) = read_to_string(base, "main.py") {
            if content.contains("FastAPI") || content.contains("uvicorn") {
                return Some(Service::new("Dev", "uvicorn main:app --reload"));
            }
            if content.contains("flask") || content.contains("Flask") {
                return Some(Service::new("Dev", "flask run"));
            }
        }
        return Some(Service::new("Dev", &format!("{} main.py", python_cmd())));
    }

    if has(base, "index.js") || has(base, "index.ts") {
        return Some(Service::new("Dev", "node index.js"));
    }
    if has(base, "server.js") {
        return Some(Service::new("Dev", "node server.js"));
    }

    if has(base, "Makefile") {
        if let Some(content) = read_to_string(base, "Makefile") {
            if content.contains("dev:") {
                return Some(Service::new("Dev", "make dev"));
            }
            if content.contains("run:") {
                return Some(Service::new("Dev", "make run"));
            }
            if content.contains("start:") {
                return Some(Service::new("Dev", "make start"));
            }
        }
        return Some(Service::new("Dev", "make"));
    }

    None
}

fn compose_cmd() -> &'static str {
    if std::process::Command::new("docker-compose")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        "docker-compose"
    } else {
        "docker compose"
    }
}

fn detect_compose(base: &Path) -> Vec<Service> {
    let content = read_to_string(base, "docker-compose.yml")
        .or_else(|| read_to_string(base, "docker-compose.yaml"))
        .or_else(|| read_to_string(base, "compose.yml"))
        .or_else(|| read_to_string(base, "compose.yaml"));

    let content = match content {
        Some(c) => c,
        None => return vec![],
    };

    let prefix = compose_cmd();

    let known: &[(&str, &str, &str)] = &[
        ("db", "Database", " up db"),
        ("database", "Database", " up database"),
        ("postgres", "Database", " up postgres"),
        ("mysql", "Database", " up mysql"),
        ("mariadb", "Database", " up mariadb"),
        ("mongodb", "MongoDB", " up mongodb"),
        ("mongo", "MongoDB", " up mongo"),
        ("redis", "Redis", " up redis"),
        ("cache", "Cache", " up cache"),
        ("memcached", "Cache", " up memcached"),
        ("mq", "Message Queue", " up mq"),
        ("rabbitmq", "RabbitMQ", " up rabbitmq"),
        ("kafka", "Kafka", " up kafka"),
        ("zookeeper", "ZooKeeper", " up zookeeper"),
        ("redpanda", "Redpanda", " up redpanda"),
        ("nats", "NATS", " up nats"),
        ("elasticsearch", "Elasticsearch", " up elasticsearch"),
        ("es", "Elasticsearch", " up es"),
        ("minio", "MinIO", " up minio"),
        ("prometheus", "Prometheus", " up prometheus"),
        ("grafana", "Grafana", " up grafana"),
        ("alertmanager", "Alertmanager", " up alertmanager"),
        ("loki", "Loki", " up loki"),
        ("tempo", "Tempo", " up tempo"),
        ("nginx", "Nginx", " up nginx"),
        ("traefik", "Traefik", " up traefik"),
        ("caddy", "Caddy", " up caddy"),
        ("vault", "Vault", " up vault"),
        ("consul", "Consul", " up consul"),
        ("keycloak", "Keycloak", " up keycloak"),
        ("solr", "Solr", " up solr"),
        ("cassandra", "Cassandra", " up cassandra"),
        ("clickhouse", "ClickHouse", " up clickhouse"),
        ("mailhog", "MailHog", " up mailhog"),
        ("mailpit", "Mailpit", " up mailpit"),
        ("adminer", "Adminer", " up adminer"),
        ("pgadmin", "pgAdmin", " up pgadmin"),
        ("phpmyadmin", "phpMyAdmin", " up phpmyadmin"),
        ("s3", "S3 (MinIO)", " up s3"),
        ("storage", "Storage", " up storage"),
    ];

    let mut services = Vec::new();
    let mut in_services = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if !in_services {
            if trimmed == "services:" {
                in_services = true;
            }
            continue;
        }
        if !line.starts_with("  ") || trimmed.is_empty() {
            continue;
        }
        if trimmed.ends_with(':') {
            let name = trimmed.trim_end_matches(':');
            if let Some(&(_, display, suffix)) = known.iter().find(|(key, _, _)| *key == name) {
                if !services.iter().any(|s: &Service| s.name == display) {
                    let cmd = format!("{}{}", prefix, suffix);
                    services.push(Service::new(display, &cmd));
                }
            }
        }
    }

    services
}

fn detect_tooling(base: &Path) -> Vec<Service> {
    let mut services = Vec::new();

    if has(base, "Dockerfile") {
        services.push(Service::new("Docker Build", "docker build -t app ."));
    }

    if has(base, "justfile") && !has(base, "Makefile") {
        services.push(Service::new("Tasks", "just"));
    }

    if has(base, "Taskfile.yml") || has(base, "Taskfile.yaml") {
        services.push(Service::new("Tasks", "task dev"));
    }

    let has_tf = std::fs::read_dir(base).ok().map_or(false, |entries| {
        entries
            .flatten()
            .any(|e| e.path().extension().map_or(false, |e| e == "tf"))
    });
    if has_tf {
        services.push(Service::new("Terraform", "terraform plan"));
    }

    if has(base, "flake.nix") {
        services.push(Service::new("Nix Shell", "nix develop"));
    }

    services
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn read_config(base: &Path) -> Vec<Service> {
    let content = read_to_string(base, "rudder.toml")
        .or_else(|| read_to_string(base, ".rudder.toml"));

    let content = match content {
        Some(c) => c,
        None => return vec![],
    };

    #[derive(serde::Deserialize)]
    struct ConfigService {
        name: String,
        cmd: String,
        dir: Option<String>,
        url: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Config {
        service: Option<Vec<ConfigService>>,
    }

    let config: Config = match toml::from_str(&content) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let services = match config.service {
        Some(s) => s,
        None => return vec![],
    };

    services
        .into_iter()
        .map(|s| {
            let mut svc = match s.dir {
                Some(d) => Service::new_with_dir(&s.name, &s.cmd, &d),
                None => Service::new(&s.name, &s.cmd),
            };
            if let Some(u) = s.url {
                svc.url = Some(u);
            }
            svc
        })
        .collect()
}

fn has_any_project_file(base: &Path) -> bool {
    has(base, "package.json")
        || has(base, "Cargo.toml")
        || has(base, "go.mod")
        || has(base, "pom.xml")
        || has(base, "build.gradle")
        || has(base, "build.gradle.kts")
        || has(base, "manage.py")
        || has(base, "pyproject.toml")
        || has(base, "requirements.txt")
        || has(base, "Gemfile")
        || has(base, "mix.exs")
        || has(base, "composer.json")
        || has(base, "Program.cs")
        || has(base, "Program.fs")
        || has(base, "main.py")
        || has(base, "index.js")
        || has(base, "index.ts")
        || has(base, "server.js")
        || has(base, "Makefile")
        || has(base, "Dockerfile")
        || has(base, "docker-compose.yml")
        || has(base, "docker-compose.yaml")
        || has(base, "compose.yml")
        || has(base, "compose.yaml")
        || has(base, "flake.nix")
        || has(base, "justfile")
        || has(base, "Taskfile.yml")
        || has(base, "Taskfile.yaml")
}

fn detect_subdirs(base: &Path) -> Vec<Service> {
    let ignore = [
        "node_modules", ".git", "target", "dist", "build",
        ".next", ".svelte-kit", ".cache", "vendor", "__pycache__",
    ];

    let mut services = Vec::new();
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return services,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        if ignore.contains(&name) {
            continue;
        }
        if name.starts_with('.') {
            continue;
        }

        let mut sub_services = Vec::new();

        if let Some(mut svc) = detect_primary(&path) {
            svc.name = capitalize(name);
            svc.dir = Some(name.to_string());
            sub_services.push(svc);
        }

        for mut svc in detect_compose(&path) {
            svc.dir = Some(name.to_string());
            sub_services.push(svc);
        }

        for mut svc in detect_tooling(&path) {
            svc.dir = Some(name.to_string());
            sub_services.push(svc);
        }

        if sub_services.is_empty() && has_any_project_file(&path) {
            let mut svc = Service::new("Dev", "npm run dev");
            svc.name = capitalize(name);
            svc.dir = Some(name.to_string());
            sub_services.push(svc);
        }

        services.extend(sub_services);
    }

    services
}

fn merge_services(base: Vec<Service>, extra: Vec<Service>) -> Vec<Service> {
    let mut result = base;
    for e in extra {
        if !result.iter().any(|s| s.name == e.name) {
            result.push(e);
        }
    }
    result
}

pub fn detect_services_for(base: &Path) -> Vec<Service> {
    let config = read_config(base);

    let mut services = Vec::new();

    if let Some(primary) = detect_primary(base) {
        services.push(primary);
    }

    services.extend(detect_compose(base));

    let subdirs = detect_subdirs(base);
    services = merge_services(services, subdirs);

    let tooling = detect_tooling(base);
    services = merge_services(services, tooling);

    services = merge_services(services, config);

    if services.is_empty() {
        services.push(Service::new("Dev", "npm run dev"));
    }

    services
}
