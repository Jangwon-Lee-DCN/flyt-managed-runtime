#include <cuda_runtime.h>
#include <cstdio>

__global__ void add_one(int *value) { *value += 1; }

int main() {
    int initial = 41;
    int result = 0;
    int *device = nullptr;
    if (cudaMalloc(&device, sizeof(int)) != cudaSuccess) return 10;
    if (cudaMemcpy(device, &initial, sizeof(int), cudaMemcpyHostToDevice) != cudaSuccess) return 11;
    add_one<<<1, 1>>>(device);
    if (cudaDeviceSynchronize() != cudaSuccess) return 12;
    if (cudaMemcpy(&result, device, sizeof(int), cudaMemcpyDeviceToHost) != cudaSuccess) return 13;
    if (cudaFree(device) != cudaSuccess) return 14;
    if (result != 42) return 15;
    std::printf("FLYT_CUDA_SMOKE_OK result=%d\n", result);
    return 0;
}
