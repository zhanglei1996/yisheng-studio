use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::AppError;

use super::ResourceClass;

/// Process-local admission control. SQLite remains the durable state authority;
/// semaphores only prevent local media/provider saturation between active runs.
pub struct ResourceScheduler {
    workflows: Arc<Semaphore>,
    cpu: Arc<Semaphore>,
    media: Arc<Semaphore>,
    network: Arc<Semaphore>,
    disk: Arc<Semaphore>,
}

impl ResourceScheduler {
    pub fn production() -> Self {
        Self::new(1, 2, 1, 2, 2)
    }

    pub fn new(workflows: usize, cpu: usize, media: usize, network: usize, disk: usize) -> Self {
        Self {
            workflows: Arc::new(Semaphore::new(workflows.max(1))),
            cpu: Arc::new(Semaphore::new(cpu.max(1))),
            media: Arc::new(Semaphore::new(media.max(1))),
            network: Arc::new(Semaphore::new(network.max(1))),
            disk: Arc::new(Semaphore::new(disk.max(1))),
        }
    }

    pub async fn acquire_workflow(&self) -> Result<OwnedSemaphorePermit, AppError> {
        acquire(self.workflows.clone()).await
    }

    pub async fn acquire(&self, class: ResourceClass) -> Result<OwnedSemaphorePermit, AppError> {
        let semaphore = match class {
            ResourceClass::Cpu => &self.cpu,
            ResourceClass::Media => &self.media,
            ResourceClass::Network => &self.network,
            ResourceClass::Disk => &self.disk,
        };
        acquire(semaphore.clone()).await
    }
}

async fn acquire(semaphore: Arc<Semaphore>) -> Result<OwnedSemaphorePermit, AppError> {
    semaphore
        .acquire_owned()
        .await
        .map_err(|_| AppError::Validation("工作流资源调度器已关闭".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resource_limit_blocks_until_the_previous_permit_is_released() {
        let scheduler = ResourceScheduler::new(1, 1, 1, 1, 1);
        let first = scheduler.acquire(ResourceClass::Media).await.unwrap();
        let second = scheduler.media.clone().try_acquire_owned();
        assert!(second.is_err());
        drop(first);
        assert!(scheduler.media.clone().try_acquire_owned().is_ok());
    }
}
