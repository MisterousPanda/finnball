import { defineRailway, project, service } from "railway/iac";

export default defineRailway(() => {
  const web = service("finnball", {
    build: {
      builder: "DOCKERFILE",
      dockerfilePath: "Dockerfile",
    },
    healthcheck: "/",
    healthcheckTimeout: 30,
    env: {
      RAILWAY_DOCKERFILE_PATH: "Dockerfile",
    },
  });

  return project("finnball", {
    resources: [web],
  });
});
