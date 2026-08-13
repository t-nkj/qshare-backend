CREATE TABLE `shared_files` (
    `id` CHAR(36) NOT NULL,
    `user_id` VARCHAR(64) NOT NULL,
    `source_device_id` CHAR(36) NULL,
    `source_device_name` VARCHAR(64) NOT NULL,
    `name` VARCHAR(1020) NOT NULL,
    `content_type` VARCHAR(255) NOT NULL,
    `size` BIGINT UNSIGNED NOT NULL,
    `storage_key` CHAR(36) NOT NULL,
    `created_at` DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    `updated_at` DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    `expires_at` DATETIME(3) NOT NULL,
    INDEX `idx_shared_files_user_history` (`user_id`, `created_at` DESC, `id` DESC),
    INDEX `idx_shared_files_user_updated` (`user_id`, `updated_at`, `id`),
    INDEX `idx_shared_files_expiry` (`expires_at`),
    UNIQUE INDEX `uq_shared_files_storage_key` (`storage_key`),
    PRIMARY KEY (`id`),
    CONSTRAINT `fk_shared_files_source_device`
        FOREIGN KEY (`source_device_id`) REFERENCES `devices` (`id`)
        ON DELETE SET NULL ON UPDATE CASCADE
) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

CREATE TABLE `shared_file_usage` (
    `user_id` VARCHAR(64) NOT NULL,
    `bytes` BIGINT UNSIGNED NOT NULL,
    PRIMARY KEY (`user_id`)
) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
