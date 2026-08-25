#include <cuda.h>
#include <cuda_runtime.h>

int get_gpu_cores(unsigned device_id) {
    struct cudaDeviceProp dev_prop;
    cudaError_t res = cudaGetDeviceProperties(&dev_prop, device_id);
    if (res != cudaSuccess) {
        return -1;
    }
    return dev_prop.multiProcessorCount;
}

int get_cuda_device_count(void) {
    int count = 0;
    cudaError_t res = cudaGetDeviceCount(&count);
    if (res != cudaSuccess) {
        return -1;
    }
    return count;
}

unsigned long long get_gpu_total_memory(unsigned device_id) {
    struct cudaDeviceProp dev_prop;
    cudaError_t res = cudaGetDeviceProperties(&dev_prop, device_id);
    if (res != cudaSuccess) {
        return 0;
    }
    return (unsigned long long)dev_prop.totalGlobalMem;
}
