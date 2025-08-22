const repository = require("./product.repository");
const Product = require("./product");
 
const handler = (product) => {
  console.log("received product:", product);
  console.log("received product event:", product.event);

  if (product.event == "CREATED" || product.event == "UPDATED") {
    return Promise.resolve(
      repository.insert(
        new Product(product.id, product.type, product.name, product.version)
      )
    );
  } else if (product.event == "DELETED") {
    return Promise.resolve(
      repository.delete(
        new Product(product.id, product.type, product.name, product.version)
      )
    );
  }
  throw new Error("Unable to process event")
};

module.exports = handler;
