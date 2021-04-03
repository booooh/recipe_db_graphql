use futures::stream::StreamExt;
use mongodb::bson::{doc, to_document};
use std::{env, error::Error, fs, io};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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
    Ok(())
}
