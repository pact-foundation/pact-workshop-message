from typing import Dict

from product.product import Product, Products
from product.product_repository import ProductRepository

repository = ProductRepository()


# Actual message handler, doesn't care about SNS at all!
async def receive_product_update(product: Dict) -> Products:
    return await repository.insert(Product(product['id'], product['type'], product['name']))
