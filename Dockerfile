FROM node:22-alpine AS builder
WORKDIR /app

COPY package*.json ./
RUN npm ci

COPY src ./src
COPY public ./public

RUN npm run build:node

FROM node:22-alpine AS runner
WORKDIR /app

ENV NODE_ENV=production
ENV PORT=38471

COPY --from=builder /app/dist ./dist
COPY public ./public

EXPOSE 38471

CMD ["node", "dist/node-server.cjs"]
