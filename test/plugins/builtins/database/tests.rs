use super::*;
use serde_json::json;

#[test]
fn the_shell_way_of_writing_json_is_accepted() {
    assert_eq!(
        mongo::strict_json("{age: {$gt: 30}, name: 'Ana', 'x.y': \"a'b\"}"),
        "{\"age\": {\"$gt\": 30}, \"name\": \"Ana\", \"x.y\": \"a'b\"}"
    );
    let filter = mongo::document("{ status: 'active', count: { $gte: 2 } }").unwrap();
    assert_eq!(filter.get_str("status").unwrap(), "active");
    assert!(mongo::document("").unwrap().is_empty());
    assert!(mongo::document("[1, 2]").is_err());
    assert!(mongo::document("{ broken").is_err());
}

#[test]
fn extended_json_goes_to_bson_and_back() {
    let text = r#"{"_id": {"$oid": "65a1b2c3d4e5f60718293a4b"}, "when": {"$date": "2024-01-02T03:04:05Z"}, "n": 3}"#;
    let document = mongo::document(text).unwrap();
    assert!(document.get_object_id("_id").is_ok());
    assert!(document.get_datetime("when").is_ok());
    let back = mongo::to_json(document);
    assert_eq!(back["_id"]["$oid"], "65a1b2c3d4e5f60718293a4b");
    assert_eq!(back["n"], 3);
}

#[test]
fn a_document_is_summed_up_on_one_line() {
    let document = json!({"_id": {"$oid": "abc"}, "name": "Ana", "tags": ["a", "b"], "address": {"city": "Lyon"}});
    assert_eq!(
        summary(&document),
        "{ _id: ObjectId(\"abc\"), name: \"Ana\", tags: [2], address: {…} }"
    );
    let long = json!({"text": "x".repeat(400)});
    assert!(summary(&long).chars().count() <= SUMMARY + 1);
}

#[test]
fn a_password_is_not_shown() {
    assert_eq!(
        hide_password("mongodb+srv://ana:secret@cluster.example.net/app"),
        "mongodb+srv://ana:•••@cluster.example.net/app"
    );
    assert_eq!(
        hide_password("mongodb://localhost:27017"),
        "mongodb://localhost:27017"
    );
}

#[test]
fn connections_are_written_and_read_back() {
    let connections = vec![Connection {
        name: "Local".into(),
        kind: Kind::MongoDb,
        uri: "mongodb://localhost:27017".into(),
    }];
    let text = serde_json::to_string(&connections).unwrap();
    assert!(text.contains("\"kind\":\"mongodb\""));
    let back: Vec<Connection> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, connections);
}

/// Against a real server: `MONGODB_URI=mongodb://localhost:27017 cargo test -- --ignored mongo`.
#[test]
#[ignore = "needs a MongoDB server, given by MONGODB_URI"]
fn mongo_reads_and_writes_a_real_server() {
    let Ok(uri) = std::env::var("MONGODB_URI") else {
        return;
    };
    assert!(!mongo::test(&uri).unwrap().is_empty());
    let collection = format!("atelier_{}", std::process::id());
    let query = |filter: &str| mongo::Query {
        database: "atelier_test".into(),
        collection: collection.clone(),
        filter: filter.into(),
        sort: "{n: 1}".into(),
        skip: 0,
        limit: 10,
    };
    mongo::insert(&uri, "atelier_test", &collection, "{name: 'a', n: 1}").unwrap();
    mongo::insert(&uri, "atelier_test", &collection, "{name: 'b', n: 2}").unwrap();
    assert!(
        mongo::databases(&uri)
            .unwrap()
            .contains(&"atelier_test".to_string())
    );
    assert!(
        mongo::collections(&uri, "atelier_test")
            .unwrap()
            .contains(&collection)
    );
    let page = mongo::find(&uri, &query("{n: {$gt: 1}}")).unwrap();
    assert_eq!(
        (page.total, page.documents[0]["name"].clone()),
        (1, json!("b"))
    );
    let shown = page.documents[0].clone();
    mongo::replace(
        &uri,
        "atelier_test",
        &collection,
        &shown,
        "{name: 'c', n: 3}",
    )
    .unwrap();
    let page = mongo::find(&uri, &query("")).unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.documents[1]["name"], "c");
    for document in page.documents {
        mongo::delete(&uri, "atelier_test", &collection, &document).unwrap();
    }
    assert_eq!(mongo::find(&uri, &query("")).unwrap().total, 0);
}

#[test]
fn errors_are_said_in_a_few_words() {
    let mongo = "Kind: Server selection timeout: No available servers. Topology: { Type: Unknown, \
                 Servers: [ { Address: localhost:27017, Type: Unknown, Error: Kind: I/O error: \
                 Aucune connexion n’a pu être établie (os error 10061) } ] }, labels: {}";
    assert_eq!(short_error(mongo), tr!("database.error_unreachable"));
    assert_eq!(
        short_error("db error: FATAL: password authentication failed for user \"x\""),
        tr!("database.error_authentication")
    );
    assert_eq!(
        short_error("MySqlError { ERROR 1045 (28000): Access denied for user 'root'@'localhost' }"),
        tr!("database.error_authentication")
    );
    assert_eq!(
        short_error(
            "error connecting to server: failed to lookup address information: No such host is known. (os error 11001)"
        ),
        tr!("database.error_host")
    );
    // Anything else: its first line, cut.
    let other = format!(
        "relation \"users\" does not exist\n{}",
        "detail ".repeat(100)
    );
    assert_eq!(short_error(&other), "relation \"users\" does not exist");
    assert!(short_error(&"x".repeat(500)).chars().count() <= 161);
}

#[test]
fn each_kind_has_its_connection_strings() {
    assert!(Kind::MongoDb.accepts("mongodb+srv://a@b/c"));
    assert!(Kind::PostgreSql.accepts("postgres://u:p@h/db"));
    assert!(Kind::PostgreSql.accepts("postgresql://h"));
    assert!(Kind::MySql.accepts("mysql://root@localhost:3306"));
    assert!(!Kind::MySql.accepts("postgres://h"));
    for kind in Kind::ALL {
        assert!(kind.accepts(kind.default_uri()));
    }
    let saved: Connection =
        serde_json::from_str(r#"{"name": "pg", "kind": "postgresql", "uri": "postgres://h"}"#)
            .unwrap();
    assert_eq!(saved.kind, Kind::PostgreSql);
}

#[test]
fn the_page_of_a_table_is_read_with_quoted_names() {
    use sql::{Dialect, page_statement, quote};
    assert_eq!(quote(Dialect::Postgres, "we\"ird"), "\"we\"\"ird\"");
    assert_eq!(quote(Dialect::MySql, "or`der"), "`or``der`");
    let (select, count) = page_statement(
        Dialect::Postgres,
        "public",
        "users",
        "age > 30;",
        "name",
        100,
        50,
    );
    assert_eq!(
        select,
        "SELECT * FROM \"public\".\"users\" WHERE age > 30 ORDER BY name LIMIT 50 OFFSET 100"
    );
    assert_eq!(
        count,
        "SELECT COUNT(*) FROM \"public\".\"users\" WHERE age > 30"
    );
    let (select, _) = page_statement(Dialect::MySql, "shop", "items", "", "", 0, 100);
    assert_eq!(select, "SELECT * FROM `shop`.`items` LIMIT 100 OFFSET 0");
}

/// Against real servers: `POSTGRES_URI=postgres://… MYSQL_URI=mysql://… cargo test -- --ignored sql`.
#[test]
#[ignore = "needs PostgreSQL or MySQL servers, given by POSTGRES_URI and MYSQL_URI"]
fn sql_reads_and_writes_real_servers() {
    for (variable, dialect, schema) in [
        ("POSTGRES_URI", sql::Dialect::Postgres, "public"),
        ("MYSQL_URI", sql::Dialect::MySql, ""),
    ] {
        let Ok(uri) = std::env::var(variable) else {
            continue;
        };
        assert!(!sql::test(dialect, &uri).unwrap().is_empty());
        let schemas = sql::schemas(dialect, &uri).unwrap();
        let schema = if schema.is_empty() {
            schemas[0].clone()
        } else {
            schema.to_string()
        };
        let table = format!("atelier_{}", std::process::id());
        let name = format!(
            "{}.{}",
            sql::quote(dialect, &schema),
            sql::quote(dialect, &table)
        );
        sql::query(
            dialect,
            &uri,
            &format!("CREATE TABLE {name} (id INT, label VARCHAR(20))"),
        )
        .unwrap();
        let inserted = sql::query(
            dialect,
            &uri,
            &format!("INSERT INTO {name} VALUES (1, 'a'), (2, NULL), (3, 'c')"),
        )
        .unwrap();
        assert_eq!(inserted.affected, Some(3));
        assert!(
            sql::tables(dialect, &uri, &schema)
                .unwrap()
                .contains(&table)
        );
        let page = sql::page(dialect, &uri, &schema, &table, "id > 1", "id DESC", 0, 10).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.grid.columns, ["id", "label"]);
        assert_eq!(
            page.grid.rows,
            vec![
                vec![Some("3".into()), Some("c".into())],
                vec![Some("2".into()), None]
            ]
        );
        sql::query(dialect, &uri, &format!("DROP TABLE {name}")).unwrap();
    }
}

#[test]
fn a_table_is_created_with_its_columns_and_primary_key() {
    use sql::{Dialect, NewColumn, create_table_statement, identity_column};
    let columns = vec![
        identity_column(Dialect::Postgres),
        NewColumn {
            name: "email".into(),
            data_type: "varchar(255)".into(),
            key: false,
            not_null: true,
            default: String::new(),
        },
        NewColumn {
            name: "active".into(),
            data_type: "boolean".into(),
            key: false,
            not_null: false,
            default: "true".into(),
        },
        // A column without a name is left out.
        NewColumn {
            name: " ".into(),
            data_type: "text".into(),
            key: false,
            not_null: false,
            default: String::new(),
        },
    ];
    assert_eq!(
        create_table_statement(Dialect::Postgres, "public", "users", &columns),
        "CREATE TABLE \"public\".\"users\" (\n  \"id\" serial NOT NULL,\n  \"email\" varchar(255) NOT NULL,\n  \"active\" boolean DEFAULT true,\n  PRIMARY KEY (\"id\")\n)"
    );
    let mysql = create_table_statement(
        Dialect::MySql,
        "shop",
        "t",
        &[identity_column(Dialect::MySql)],
    );
    assert_eq!(
        mysql,
        "CREATE TABLE `shop`.`t` (\n  `id` INT AUTO_INCREMENT NOT NULL,\n  PRIMARY KEY (`id`)\n)"
    );
    assert_eq!(
        sql::create_schema_statement(Dialect::MySql, "shop"),
        "CREATE DATABASE `shop`"
    );
    assert_eq!(
        sql::drop_schema_statement(Dialect::Postgres, "app"),
        "DROP SCHEMA \"app\" CASCADE"
    );
}

#[test]
fn rows_are_changed_by_their_primary_key() {
    use sql::{ColumnInfo, Dialect, RowRef, insert_statement};
    let columns = vec![
        ColumnInfo {
            name: "id".into(),
            data_type: "integer".into(),
            nullable: false,
            default: None,
            key: true,
        },
        ColumnInfo {
            name: "name".into(),
            data_type: "text".into(),
            nullable: true,
            default: None,
            key: false,
        },
    ];
    let names = vec!["id".to_string(), "name".to_string()];
    let row = vec![Some("7".to_string()), Some("O'Brien".to_string())];
    let reference = RowRef {
        dialect: Dialect::Postgres,
        schema: "public",
        table: "people",
        columns: &columns,
        names: &names,
        row: &row,
    };
    assert_eq!(
        reference.update("name", &Some("Ann".into())).unwrap(),
        "UPDATE \"public\".\"people\" SET \"name\" = 'Ann' WHERE \"id\" = '7'"
    );
    assert_eq!(
        reference.update("name", &None).unwrap(),
        "UPDATE \"public\".\"people\" SET \"name\" = NULL WHERE \"id\" = '7'"
    );
    assert_eq!(
        reference.delete().unwrap(),
        "DELETE FROM \"public\".\"people\" WHERE \"id\" = '7'"
    );
    // Without a primary key, a row cannot be told apart.
    let no_key: Vec<ColumnInfo> = columns
        .iter()
        .map(|c| ColumnInfo {
            key: false,
            ..c.clone()
        })
        .collect();
    let reference = RowRef {
        columns: &no_key,
        ..reference
    };
    assert!(reference.delete().is_none());
    assert_eq!(
        insert_statement(
            Dialect::MySql,
            "shop",
            "people",
            &[
                ("name".into(), Some("O'Brien".into())),
                ("note".into(), None)
            ]
        ),
        "INSERT INTO `shop`.`people` (`name`, `note`) VALUES ('O''Brien', NULL)"
    );
    assert_eq!(
        insert_statement(Dialect::Postgres, "public", "people", &[]),
        "INSERT INTO \"public\".\"people\" DEFAULT VALUES"
    );
}
