FROM rust AS BUILD

WORKDIR /app

RUN rustup target add wasm32-unknown-unknown
RUN wget -O- https://github.com/trunk-rs/trunk/releases/download/v0.20.3/trunk-x86_64-unknown-linux-gnu.tar.gz | tar -zxv

COPY . /app
RUN ./trunk build

FROM nginx:stable-alpine

RUN rm /usr/share/nginx/html/*
COPY --from=BUILD /app/dist /usr/share/nginx/html/
COPY nginx.conf /etc/nginx/nginx.conf

EXPOSE 80
