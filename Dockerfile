# Serve the Bevy WebGL client. Do not compile the native game on Railway —
# there is no GPU, and a Bevy window cannot run as a headless web service.
FROM nginx:1.27-alpine

COPY deploy/nginx.default.conf /etc/nginx/conf.d/default.conf
COPY deploy/start-nginx.sh /start-nginx.sh
COPY www /usr/share/nginx/html

RUN chmod +x /start-nginx.sh \
    && rm -f /usr/share/nginx/html/vercel.json

EXPOSE 8080
CMD ["/start-nginx.sh"]
