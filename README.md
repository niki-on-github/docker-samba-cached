# SAMBA Container

Samba Server in container with read cache.

## Features
- **RAM Cache**: Uses inotify + vmtouch to cache video files in RAM
- **30s Cooldown**: Prevents re-caching files within 30 seconds of last cache
- **Recursive Monitoring**: Watches all subdirectories for video files
