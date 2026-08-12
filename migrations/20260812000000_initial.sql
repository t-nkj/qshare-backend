CREATE TABLE IF NOT EXISTS `devices` (
    `id` CHAR(36) NOT NULL,
    `user_id` VARCHAR(64) NOT NULL,
    `name` VARCHAR(64) NOT NULL,
    `token_hash` BINARY(32) NOT NULL,
    `created_at` DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    `updated_at` DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    `last_used_at` DATETIME(3) NULL,

    UNIQUE INDEX `uq_devices_token_hash` (`token_hash`),
    INDEX `idx_devices_user` (`user_id`, `created_at`, `id`),
    PRIMARY KEY (`id`)
) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `shared_urls` (
    `id` CHAR(36) NOT NULL,
    `user_id` VARCHAR(64) NOT NULL,
    `source_device_id` CHAR(36) NULL,
    `source_device_name` VARCHAR(64) NOT NULL,
    `url` VARCHAR(4096) NOT NULL,
    `created_at` DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    `expires_at` DATETIME(3) NOT NULL,

    INDEX `idx_shared_urls_user_history` (`user_id`, `created_at` DESC, `id` DESC),
    INDEX `idx_shared_urls_expiry` (`expires_at`),
    PRIMARY KEY (`id`),
    CONSTRAINT `fk_shared_urls_source_device`
        FOREIGN KEY (`source_device_id`) REFERENCES `devices` (`id`)
        ON DELETE SET NULL ON UPDATE CASCADE
) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
