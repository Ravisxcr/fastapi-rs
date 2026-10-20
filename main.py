from fastapi_rs import FastAPI

app = FastAPI()

@app.get("/hello")
def hello():
    return {"message": "Hello from fastapi-rs!"}

if __name__ == "__main__":
    print("Dispatch /hello:", app.dispatch("/hello"))
