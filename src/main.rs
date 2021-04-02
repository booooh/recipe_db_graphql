use std::{env, fmt};
use std::{error::Error, sync::Arc};
use std::{fs, io};

use actix_web::{middleware, web, HttpResponse};
use futures::stream::StreamExt;
use mongodb::{
    bson::{self, doc, to_document},
    Collection,
};
use serde::{Deserialize, Serialize};
use serde_json;

use tokio;

use juniper::{
    graphql_object,
    http::{graphiql::graphiql_source, GraphQLRequest},
    EmptyMutation, EmptySubscription, FieldError, FieldResult, GraphQLEnum, GraphQLInputObject,
    GraphQLObject, IntoFieldError, ScalarValue,
};

#[derive(Serialize, Deserialize, GraphQLObject)]
#[graphql(description = "An ingredient used in a recipe")]
struct Ingredient {
    name: String,
    qty: String,
}

#[derive(Serialize, Deserialize, GraphQLObject)]
#[graphql(description = "A reference to some Media in the recipe")]
struct MediaRef {
    anchor: String,
    url: String,
}

#[derive(Serialize, Deserialize, GraphQLObject)]
#[graphql(description = "A recipe")]
struct Recipe {
    title: String,
    ingredients: Vec<Ingredient>,
    instructions: Vec<String>,
    tags: Vec<String>,
    media: Vec<MediaRef>,
}

struct Context {
    collection: Collection,
}

// Error type

#[derive(Debug, Clone)]
pub enum AppErrorType {
    DbError,
    #[allow(dead_code)]
    NotFoundError,
    InvalidField,
    IOError,
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub message: Option<String>,
    pub cause: Option<String>,
    pub error_type: AppErrorType,
}

impl AppError {
    pub fn message(&self) -> String {
        match &*self {
            AppError {
                message: Some(message),
                ..
            } => message.clone(),
            AppError {
                error_type: AppErrorType::NotFoundError,
                ..
            } => "The requested item was not found".to_string(),
            AppError {
                error_type: AppErrorType::InvalidField,
                ..
            } => "Invalid field value provided".to_string(),
            _ => "An unexpected error has occurred".to_string(),
        }
    }
}

impl<S> IntoFieldError<S> for AppError
where
    S: ScalarValue,
{
    fn into_field_error(self) -> FieldError<S> {
        FieldError::new(self.message(), juniper::Value::null())
    }
}

impl From<mongodb::error::Error> for AppError {
    fn from(error: mongodb::error::Error) -> AppError {
        AppError {
            message: None,
            cause: Some(error.to_string()),
            error_type: AppErrorType::DbError,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        AppError {
            message: None,
            cause: Some(error.to_string()),
            error_type: AppErrorType::IOError,
        }
    }
}

impl From<bson::de::Error> for AppError {
    fn from(error: bson::de::Error) -> AppError {
        AppError {
            message: None,
            cause: Some(error.to_string()),
            error_type: AppErrorType::DbError,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "{}", self.message())
    }
}

// To make our context usable by Juniper, we have to implement a marker trait.
impl juniper::Context for Context {}

struct Query;

#[graphql_object(
    // Here we specify the context type for the object.
    // We need to do this in every type that
    // needs access to the context.
    context = Context,
)]
impl Query {
    fn api_version() -> &'static str {
        "0.1"
    }

    // Arguments to resolvers can either be simple types or input objects.
    // To gain access to the context, we specify a argument
    // that is a reference to the Context type.
    // Juniper automatically injects the correct context here.
    async fn recipes(context: &Context) -> Result<Vec<Recipe>, AppError> {
        let mut recipes = Vec::<Recipe>::new();
        let mut cursor = context.collection.find(doc! {}, None).await?;
        while let Some(doc) = cursor.next().await {
            let recipe = bson::from_document(doc?)?;
            recipes.push(recipe);
        }
        Ok(recipes)
    }
}

// A root schema consists of a query and a mutation.
// Request queries can be executed against a RootNode.
type Schema = juniper::RootNode<
    'static,
    Query,
    juniper::EmptyMutation<Context>,
    juniper::EmptySubscription<Context>,
>;

fn create_schema() -> Schema {
    Schema::new(
        Query,
        juniper::EmptyMutation::new(),
        juniper::EmptySubscription::new(),
    )
}

async fn graphql_playground() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(graphiql_source("/graphql", None))
}

async fn graphql(
    schema: web::Data<Schema>,
    data: web::Json<GraphQLRequest>,
    collection: web::Data<Collection>,
) -> HttpResponse {
    let ctx = Context {
        collection: collection.get_ref().to_owned(),
    };
    let res = data.execute(&schema, &ctx).await;

    HttpResponse::Ok().json(res)
}

fn register(config: &mut web::ServiceConfig) {
    config
        .data(create_schema())
        .route("/graphql", web::post().to(graphql))
        .route("/graphiql", web::get().to(graphql_playground));
}

#[actix_web::main]
async fn main() -> Result<(), AppError> {
    //dotenv::dotenv().ok();
    std::env::set_var("RUST_LOG", "actix_web=info,info");
    env_logger::init();

    // Load the MongoDB connection string from an environment variable:
    let client_uri =
        env::var("MONGODB_URI").expect("You must set the MONGODB_URI environment var!");

    // A Client is needed to connect to MongoDB:
    let client = mongodb::Client::with_uri_str(client_uri.as_ref()).await?;

    let database = client.database("recipedb");
    let collection = database.collection("recipes");

    actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .data(collection.clone())
            .wrap(middleware::Logger::default())
            .configure(register)
            .default_service(web::to(|| async { "404" }))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
    .map_err(AppError::from)
}

//#[tokio::main]
async fn main2() -> Result<(), Box<dyn Error>> {
    // Load the MongoDB connection string from an environment variable:
    let client_uri =
        env::var("MONGODB_URI").expect("You must set the MONGODB_URI environment var!");

    // A Client is needed to connect to MongoDB:
    let client = mongodb::Client::with_uri_str(client_uri.as_ref()).await?;

    let database = client.database("recipedb");
    let collection = database.collection("recipes");

    // drop all current documents from the collection
    collection.delete_many(doc! {}, None).await?;

    // insert all recipes from the json data to the DB:
    let mut entries = fs::read_dir("./data")?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort();

    // for each file, upload all recipes
    for entry in entries {
        let s = fs::read_to_string(entry).unwrap();
        let recipes: serde_json::Value = serde_json::from_str(&s).unwrap();
        let docs = recipes
            .as_array()
            .unwrap()
            .iter()
            .map(|r| to_document(r).unwrap())
            .collect::<Vec<_>>();
        collection.insert_many(docs, None).await?;
    }

    // iterate over all of the documents found
    let mut cursor = collection.find(None, None).await?;
    while let Some(doc) = cursor.next().await {
        println!("{}", doc?)
    }
    let ctx = Context { collection };
    let (q, _err) = juniper::execute(
        "query {recipes {title}}",
        None,
        &create_schema(),
        &juniper::Variables::new(),
        &ctx,
    )
    .await
    .unwrap();
    println!("{}", q);
    Ok(())
}
