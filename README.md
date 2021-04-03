Example for working with mongodb and graphql in rust
=====================================================

1. Start mongodb in a container (mapping host port to default container port)
```bash
docker run --rm --name mongodb -p 27017:27017 bitnami/mongodb:4.4.4
```
2. Run the demo application, which drops all data from the default collection, re-loads it from the json files located in `data/` and then serves it via graphql
```bash
export MONGODB_URI=mongodb://localhost:27017
cargo run --bin recipe_loader
```

3. Run the actix-web server which exposes a graphql endpoint and a graphiql playground to execute queries
```bash
caro run --bin server
```
