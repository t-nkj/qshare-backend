CREATE TABLE `shared_memos` (
    `id` CHAR(36) NOT NULL,
    `user_id` VARCHAR(64) NOT NULL,
    `source_device_id` CHAR(36) NULL,
    `source_device_name` VARCHAR(64) NOT NULL,
    `content` TEXT NOT NULL,
    `created_at` DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    `updated_at` DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    `expires_at` DATETIME(3) NOT NULL,

    INDEX `idx_shared_memos_user_history` (`user_id`, `created_at` DESC, `id` DESC),
    INDEX `idx_shared_memos_expiry` (`expires_at`),
    PRIMARY KEY (`id`),
    CONSTRAINT `fk_shared_memos_source_device`
        FOREIGN KEY (`source_device_id`) REFERENCES `devices` (`id`)
        ON DELETE SET NULL ON UPDATE CASCADE
) DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
