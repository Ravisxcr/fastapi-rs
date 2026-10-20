import uvicorn
from fastapi_rs import FastAPI

app = FastAPI()

@app.get("/hello")
def hello():
    return {"message": "Hello from fastapi-rs!"}

@app.get("/async-hello")
async def async_hello():
    return {"message": "Hello from async fastapi-rs!"}

if __name__ == "__main__":
    uvicorn.run("main:app", host="127.0.0.1", port=8000, reload=True)
